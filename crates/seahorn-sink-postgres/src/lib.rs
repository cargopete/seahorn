use std::time::Duration;

use anyhow::Result;
use seahorn_core::{ChangeSet, Cursor, EntityChange, Sink, Step, Value};
use serde_json::{Map, Value as Json};
use sqlx::{PgPool, Row};

#[cfg(test)]
mod tests;

pub struct PostgresSink {
    pub(crate) pool: PgPool,
    cursor_name: String,
}

impl PostgresSink {
    pub async fn connect(database_url: &str, cursor_name: impl Into<String>) -> Result<Self> {
        let pool = PgPool::connect(database_url).await?;
        sqlx::migrate!().run(&pool).await?;
        tracing::info!("PostgresSink ready — migrations applied");
        Ok(Self { pool, cursor_name: cursor_name.into() })
    }

    /// Returns the last persisted cursor, or `None` if this is a fresh start.
    pub async fn load_cursor(&self) -> Result<Option<Cursor>> {
        let row = sqlx::query("SELECT cursor_bytes FROM cursors WHERE name = $1")
            .bind(&self.cursor_name)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| {
            let bytes: Vec<u8> = r.get("cursor_bytes");
            Cursor(bytes)
        }))
    }

    /// Spawns a background task that periodically promotes confirmed rows to FINAL
    /// by querying the Solana RPC for the current finalized slot.
    ///
    /// The handle is detached — it runs until the process exits.
    pub fn start_sweeper(&self, rpc_url: String) {
        let pool = self.pool.clone();
        tokio::spawn(async move {
            let client = reqwest::Client::new();
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;
                match fetch_finalized_slot(&client, &rpc_url).await {
                    Ok(slot) => {
                        match promote_to_final(&pool, slot).await {
                            Ok(0) => {}
                            Ok(rows) => tracing::info!(slot, rows, "sweeper: promoted to FINAL"),
                            Err(e) => tracing::warn!("sweeper db error: {e}"),
                        }
                    }
                    Err(e) => tracing::warn!("sweeper rpc error: {e}"),
                }
            }
        });
    }
}

impl Sink for PostgresSink {
    async fn apply(&self, cs: &ChangeSet) -> Result<()> {
        let sig = bs58::encode(&cs.cursor.0).into_string();

        match cs.step {
            Step::New | Step::Undo => {
                let status = if cs.step == Step::New { "NEW" } else { "UNDO" };
                let mut txn = self.pool.begin().await?;

                for change in &cs.changes {
                    match change {
                        EntityChange::Upsert { entity_type, id, fields } => {
                            let fields_json = fields_to_json(fields);
                            sqlx::query(
                                "INSERT INTO entity_changes \
                                 (entity_type, entity_id, slot, tx_signature, commitment_status, fields) \
                                 VALUES ($1, $2, $3, $4, $5, $6)",
                            )
                            .bind(*entity_type)
                            .bind(id)
                            .bind(cs.slot as i64)
                            .bind(&sig)
                            .bind(status)
                            .bind(&fields_json)
                            .execute(&mut *txn)
                            .await?;
                        }
                        EntityChange::Delete { entity_type, id } => {
                            sqlx::query(
                                "INSERT INTO entity_changes \
                                 (entity_type, entity_id, slot, tx_signature, commitment_status, fields) \
                                 VALUES ($1, $2, $3, $4, $5, NULL)",
                            )
                            .bind(*entity_type)
                            .bind(id)
                            .bind(cs.slot as i64)
                            .bind(&sig)
                            .bind(status)
                            .execute(&mut *txn)
                            .await?;
                        }
                    }
                }

                upsert_cursor(&mut txn, &self.cursor_name, &cs.cursor, cs.slot).await?;
                txn.commit().await?;
                tracing::debug!(slot = cs.slot, status, changes = cs.changes.len(), "applied changeset");
            }

            Step::Irreversible => {
                let mut txn = self.pool.begin().await?;

                // Promote all NEW rows at this slot to FINAL.
                let rows = sqlx::query(
                    "UPDATE entity_changes SET commitment_status = 'FINAL' \
                     WHERE slot = $1 AND commitment_status = 'NEW'",
                )
                .bind(cs.slot as i64)
                .execute(&mut *txn)
                .await?
                .rows_affected();

                upsert_cursor(&mut txn, &self.cursor_name, &cs.cursor, cs.slot).await?;
                txn.commit().await?;
                tracing::debug!(slot = cs.slot, rows, "finalized slot");
            }
        }

        Ok(())
    }
}

async fn fetch_finalized_slot(client: &reqwest::Client, rpc_url: &str) -> Result<u64> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSlot",
        "params": [{"commitment": "finalized"}]
    });

    // Extract basic auth credentials embedded in the URL (user:pass@host),
    // then strip them before sending — reqwest does not do this automatically.
    let parsed = url::Url::parse(rpc_url)?;
    let user = parsed.username().to_string();
    let pass = parsed.password().map(str::to_string);
    let mut clean = parsed.clone();
    clean.set_username("").ok();
    clean.set_password(None).ok();

    let mut builder = client.post(clean.as_str()).json(&body);
    if !user.is_empty() {
        builder = builder.basic_auth(user, pass);
    }

    let resp: serde_json::Value = builder.send().await?.json().await?;

    resp["result"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("unexpected RPC response: {resp}"))
}

async fn promote_to_final(pool: &PgPool, finalized_slot: u64) -> Result<u64> {
    let rows = sqlx::query(
        "UPDATE entity_changes SET commitment_status = 'FINAL' \
         WHERE slot <= $1 AND commitment_status = 'NEW'",
    )
    .bind(finalized_slot as i64)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(rows)
}

async fn upsert_cursor(
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    name: &str,
    cursor: &Cursor,
    slot: u64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO cursors (name, cursor_bytes, slot) VALUES ($1, $2, $3)
         ON CONFLICT (name) DO UPDATE SET cursor_bytes = $2, slot = $3, updated_at = now()",
    )
    .bind(name)
    .bind(&cursor.0)
    .bind(slot as i64)
    .execute(&mut **txn)
    .await?;
    Ok(())
}

fn fields_to_json(fields: &[(&'static str, Value)]) -> Json {
    let mut map = Map::new();
    for (k, v) in fields {
        let json_val = match v {
            Value::String(s) => Json::String(s.clone()),
            Value::U64(n)    => Json::Number((*n).into()),
            Value::I64(n)    => Json::Number((*n).into()),
            Value::Bool(b)   => Json::Bool(*b),
            Value::Bytes(b)  => Json::String(bs58::encode(b).into_string()),
            Value::Null      => Json::Null,
        };
        map.insert(k.to_string(), json_val);
    }
    Json::Object(map)
}

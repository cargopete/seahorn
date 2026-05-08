use anyhow::Result;
use seahorn_core::{ChangeSet, Cursor, EntityChange, Sink, Step, Value};
use serde_json::{Map, Value as Json};
use sqlx::{PgPool, Row};

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS entity_changes (
    id               BIGSERIAL PRIMARY KEY,
    entity_type      TEXT        NOT NULL,
    entity_id        TEXT        NOT NULL,
    slot             BIGINT      NOT NULL,
    tx_signature     TEXT,
    commitment_status TEXT       NOT NULL,
    fields           JSONB,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_ec_type_slot ON entity_changes (entity_type, slot);
CREATE INDEX IF NOT EXISTS idx_ec_slot      ON entity_changes (slot);
CREATE INDEX IF NOT EXISTS idx_ec_status    ON entity_changes (commitment_status);

CREATE TABLE IF NOT EXISTS cursors (
    name         TEXT PRIMARY KEY,
    cursor_bytes BYTEA NOT NULL,
    slot         BIGINT NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
"#;

pub struct PostgresSink {
    pool: PgPool,
}

impl PostgresSink {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPool::connect(database_url).await?;
        sqlx::query(SCHEMA_SQL).execute(&pool).await?;
        tracing::info!("PostgresSink ready — schema ensured");
        Ok(Self { pool })
    }

    /// Returns the last persisted cursor, or `None` if this is a fresh start.
    pub async fn load_cursor(&self) -> Result<Option<Cursor>> {
        let row = sqlx::query("SELECT cursor_bytes FROM cursors WHERE name = 'default'")
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| {
            let bytes: Vec<u8> = r.get("cursor_bytes");
            Cursor(bytes)
        }))
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

                upsert_cursor(&mut txn, &cs.cursor, cs.slot).await?;
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

                upsert_cursor(&mut txn, &cs.cursor, cs.slot).await?;
                txn.commit().await?;
                tracing::debug!(slot = cs.slot, rows, "finalized slot");
            }
        }

        Ok(())
    }
}

async fn upsert_cursor(
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    cursor: &Cursor,
    slot: u64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO cursors (name, cursor_bytes, slot) VALUES ('default', $1, $2)
         ON CONFLICT (name) DO UPDATE SET cursor_bytes = $1, slot = $2, updated_at = now()",
    )
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

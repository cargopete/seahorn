use anyhow::{Context, Result};
use clap::Parser;
use futures::StreamExt;
use seahorn_core::{ChangeSet, Cursor, EntityChange, Handler, Sink, Step, Substrate, SubstrateEvent, Value};
use seahorn_handler_pumpfun::{PumpfunHandler, PUMPFUN_PROGRAM_ID};
use seahorn_sink_postgres::PostgresSink;
use seahorn_substrate_mock::PumpfunMockSubstrate;
use tonic::{
    transport::{Channel, ClientTlsConfig},
    Request,
};
use yellowstone_grpc_proto::prelude::{
    geyser_client::GeyserClient, subscribe_update::UpdateOneof, CommitmentLevel,
    SubscribeRequest, SubscribeRequestFilterTransactions,
};

#[derive(Parser)]
#[command(name = "seahorn", about = "Solana data service — Seahorn")]
struct Cli {
    /// Use synthetic Pump.fun data instead of a live Yellowstone endpoint
    #[arg(long)]
    mock: bool,

    /// Write to Postgres instead of stdout (reads DATABASE_URL from env)
    #[arg(long)]
    postgres: bool,
}

// ── Runtime loop ──────────────────────────────────────────────────────────────

async fn run<S, H, K>(substrate: S, handler: H, sink: K) -> Result<()>
where
    S: Substrate,
    H: Handler,
    K: Sink,
{
    let mut stream = std::pin::pin!(substrate.stream(None));
    while let Some(result) = stream.next().await {
        let event = result?;
        let changeset = handler.handle(&event);
        if !changeset.is_empty() {
            sink.apply(&changeset).await?;
        }
    }
    Ok(())
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "seahorn=info".into()),
        )
        .init();

    let cli = Cli::parse();
    let handler = PumpfunHandler;

    if cli.postgres {
        let db_url = std::env::var("DATABASE_URL")
            .context("DATABASE_URL not set — required for --postgres")?;
        let sink = PostgresSink::connect(&db_url).await?;

        if cli.mock {
            tracing::info!("Mock substrate → PostgresSink");
            run(PumpfunMockSubstrate::default(), handler, sink).await
        } else {
            tracing::info!("Yellowstone substrate → PostgresSink");
            run(yellowstone_substrate()?, handler, sink).await
        }
    } else {
        let sink = StdoutSink;

        if cli.mock {
            tracing::info!("Mock substrate — synthetic Pump.fun events");
            tracing::info!("(set YELLOWSTONE_ENDPOINT in .env to switch to live data)\n");
            run(PumpfunMockSubstrate::default(), handler, sink).await
        } else {
            run(yellowstone_substrate()?, handler, sink).await
        }
    }
}

// ── Stdout sink (dev) ─────────────────────────────────────────────────────────

struct StdoutSink;

impl Sink for StdoutSink {
    async fn apply(&self, cs: &ChangeSet) -> Result<()> {
        let step = match cs.step {
            Step::New          => "NEW  ",
            Step::Undo         => "UNDO ",
            Step::Irreversible => "FINAL",
        };

        for change in &cs.changes {
            let EntityChange::Upsert { entity_type, id: _, fields } = change else {
                continue;
            };

            let get = |key: &str| -> String {
                fields.iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| match v {
                        Value::String(s) => s.clone(),
                        Value::U64(n) => n.to_string(),
                        _ => String::new(),
                    })
                    .unwrap_or_default()
            };

            match *entity_type {
                "Buy" => {
                    let sol = get("sol_cost").parse::<u64>().unwrap_or(0);
                    println!(
                        "[slot {:>12}] [{step}] 🟢 Buy    mint={}…  user={}…  tokens={:>14}  sol={:.4}",
                        cs.slot,
                        &get("mint")[..8],
                        &get("user")[..8],
                        get("token_amount"),
                        sol as f64 / 1_000_000_000.0,
                    );
                }
                "Sell" => {
                    let sol = get("sol_output").parse::<u64>().unwrap_or(0);
                    println!(
                        "[slot {:>12}] [{step}] 🔴 Sell   mint={}…  user={}…  tokens={:>14}  sol={:.4}",
                        cs.slot,
                        &get("mint")[..8],
                        &get("user")[..8],
                        get("token_amount"),
                        sol as f64 / 1_000_000_000.0,
                    );
                }
                "Create" => {
                    println!(
                        "[slot {:>12}] [{step}] ✨ Create mint={}…  name={:12}  sym={}  creator={}…",
                        cs.slot,
                        &get("mint")[..8],
                        get("name"),
                        get("symbol"),
                        &get("creator")[..8],
                    );
                }
                _ => {}
            }
        }
        Ok(())
    }
}

// ── Yellowstone substrate (live) ──────────────────────────────────────────────

struct YellowstoneSubstrate {
    endpoint: String,
    token: Option<String>,
}

fn yellowstone_substrate() -> Result<YellowstoneSubstrate> {
    let endpoint = std::env::var("YELLOWSTONE_ENDPOINT")
        .context("YELLOWSTONE_ENDPOINT not set — use --mock for local dev")?;
    let token = std::env::var("YELLOWSTONE_TOKEN").ok();
    Ok(YellowstoneSubstrate { endpoint, token })
}

impl Substrate for YellowstoneSubstrate {
    fn stream(
        &self,
        _from: Option<Cursor>,
    ) -> impl futures::Stream<Item = Result<SubstrateEvent>> + Send + '_ {
        async_stream::stream! {
            let tls = ClientTlsConfig::new().with_native_roots();
            let channel = Channel::from_shared(self.endpoint.clone())
                .map_err(anyhow::Error::from)?
                .tls_config(tls).map_err(anyhow::Error::from)?
                .connect().await.context("failed to connect to Yellowstone")?;

            let token = self.token.clone();
            let mut client = GeyserClient::with_interceptor(channel, move |mut req: Request<()>| {
                if let Some(ref t) = token {
                    if let Ok(val) = t.parse() { req.metadata_mut().insert("x-token", val); }
                }
                Ok(req)
            });

            let mut filters = std::collections::HashMap::new();
            filters.insert("pumpfun".to_string(), SubscribeRequestFilterTransactions {
                account_include: vec![PUMPFUN_PROGRAM_ID.to_string()],
                vote: Some(false),
                failed: Some(false),
                ..Default::default()
            });

            let request = SubscribeRequest {
                transactions: filters,
                commitment: Some(CommitmentLevel::Confirmed as i32),
                ..Default::default()
            };

            let mut stream = match client.subscribe(tokio_stream::once(request)).await {
                Ok(r) => r.into_inner(),
                Err(e) => { yield Err(e.into()); return; }
            };

            tracing::info!("Yellowstone stream open — watching Pump.fun\n");

            while let Some(msg) = stream.next().await {
                let update = match msg {
                    Ok(u) => u,
                    Err(e) => { yield Err(e.into()); break; }
                };

                let Some(UpdateOneof::Transaction(tx_update)) = update.update_oneof else { continue };
                let Some(tx_info) = tx_update.transaction else { continue };
                let Some(tx) = tx_info.transaction else { continue };
                let Some(msg) = tx.message else { continue };

                let slot = tx_update.slot;
                let account_keys: Vec<Vec<u8>> = msg.account_keys.clone();

                let instructions = msg.instructions.iter().map(|ix| {
                    seahorn_core::RawInstruction {
                        program_id: account_keys.get(ix.program_id_index as usize).cloned().unwrap_or_default(),
                        data: ix.data.clone(),
                        accounts: ix.accounts.iter()
                            .map(|&i| account_keys.get(i as usize).cloned().unwrap_or_default())
                            .collect(),
                    }
                }).collect();

                yield Ok(SubstrateEvent {
                    slot,
                    signature: tx_info.signature.clone(),
                    step: Step::New,
                    cursor: Cursor(slot.to_le_bytes().to_vec()),
                    instructions,
                });
            }
        }
    }
}

use anyhow::{Context, Result};
use std::time::Duration;
use clap::Parser;
use futures::StreamExt;
use seahorn_core::{
    ChangeSet, Cursor, EntityChange, Handler, MultiHandler, Sink, Step, Substrate, SubstrateEvent,
    Value,
};
use seahorn_handler_pumpfun::{PumpfunHandler, PUMPFUN_PROGRAM_ID};
use seahorn_handler_jupiter::{JupiterV6Handler, JUPITER_V6_PROGRAM_ID};
use seahorn_handler_raydium::{RaydiumClmmHandler, RAYDIUM_CLMM_PROGRAM_ID};
use seahorn_sink_postgres::PostgresSink;
use seahorn_substrate_mock::{AllProgramsMockSubstrate, JupiterV6MockSubstrate, PumpfunMockSubstrate, RaydiumClmmMockSubstrate};
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
    /// Use synthetic data instead of a live Yellowstone endpoint
    #[arg(long)]
    mock: bool,

    /// Write to Postgres instead of stdout (reads DATABASE_URL from env)
    #[arg(long)]
    postgres: bool,

    /// Index Raydium CLMM instead of Pump.fun
    #[arg(long)]
    raydium: bool,

    /// Index Jupiter v6 instead of Pump.fun
    #[arg(long)]
    jupiter: bool,

    /// Index all programs simultaneously (Pump.fun + Raydium CLMM + Jupiter v6)
    #[arg(long)]
    all: bool,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn log_cursor(from: &Option<Cursor>) {
    if let Some(c) = from {
        let slot = u64::from_le_bytes(c.0.as_slice().try_into().unwrap_or([0u8; 8]));
        tracing::info!(slot, "resuming from persisted cursor");
    }
}

// ── Runtime loop ──────────────────────────────────────────────────────────────

async fn run<S, H, K>(substrate: S, handler: H, sink: K, from: Option<Cursor>) -> Result<()>
where
    S: Substrate,
    H: Handler,
    K: Sink,
{
    let mut stream = std::pin::pin!(substrate.stream(from));
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

    // Build handler — single or multi
    let handler: Box<dyn Handler> = if cli.all {
        Box::new(MultiHandler::new(vec![
            Box::new(PumpfunHandler),
            Box::new(RaydiumClmmHandler),
            Box::new(JupiterV6Handler),
        ]))
    } else if cli.raydium {
        Box::new(RaydiumClmmHandler)
    } else if cli.jupiter {
        Box::new(JupiterV6Handler)
    } else {
        Box::new(PumpfunHandler)
    };

    if cli.postgres {
        let db_url = std::env::var("DATABASE_URL")
            .context("DATABASE_URL not set — required for --postgres")?;
        let sink = PostgresSink::connect(&db_url).await?;

        if let Ok(rpc_url) = std::env::var("SOLANA_RPC_URL") {
            tracing::info!("finalization sweeper active");
            sink.start_sweeper(rpc_url);
        } else {
            tracing::info!("SOLANA_RPC_URL not set — finalization sweeper disabled");
        }

        if cli.mock {
            let from = sink.load_cursor().await?;
            log_cursor(&from);
            if cli.jupiter {
                tracing::info!("Jupiter v6 mock → PostgresSink");
                run(JupiterV6MockSubstrate::default(), &handler, &sink, from).await
            } else if cli.all {
                tracing::info!("all programs mock → PostgresSink");
                run(AllProgramsMockSubstrate::default(), &handler, &sink, from).await
            } else if cli.raydium {
                tracing::info!("Raydium CLMM mock → PostgresSink");
                run(RaydiumClmmMockSubstrate::default(), &handler, &sink, from).await
            } else {
                tracing::info!("Pump.fun mock → PostgresSink");
                run(PumpfunMockSubstrate::default(), &handler, &sink, from).await
            }
        } else {
            // Yellowstone: reconnect with exponential backoff, reload cursor on each attempt.
            tracing::info!("Yellowstone substrate → PostgresSink");
            let mut backoff = Duration::from_secs(1);
            loop {
                let from = sink.load_cursor().await?;
                log_cursor(&from);
                match run(yellowstone_substrate(&cli)?, &handler, &sink, from).await {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        tracing::warn!(error = %e, next_retry_secs = backoff.as_secs(), "stream error — reconnecting");
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(60));
                    }
                }
            }
        }
    } else {
        let sink = StdoutSink;

        if cli.mock {
            if cli.jupiter {
                tracing::info!("Jupiter v6 mock — synthetic events\n");
                run(JupiterV6MockSubstrate::default(), handler, sink, None).await
            } else if cli.all {
                tracing::info!("all programs mock — synthetic events\n");
                run(AllProgramsMockSubstrate::default(), handler, sink, None).await
            } else if cli.raydium {
                tracing::info!("Raydium CLMM mock — synthetic events\n");
                run(RaydiumClmmMockSubstrate::default(), handler, sink, None).await
            } else {
                tracing::info!("Pump.fun mock — synthetic events");
                tracing::info!("(set YELLOWSTONE_ENDPOINT in .env to switch to live data)\n");
                run(PumpfunMockSubstrate::default(), handler, sink, None).await
            }
        } else {
            run(yellowstone_substrate(&cli)?, handler, sink, None).await
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
            let EntityChange::Upsert { entity_type, id: _, fields } = change else { continue };

            let get = |key: &str| -> String {
                fields.iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| match v {
                        Value::String(s) => s.clone(),
                        Value::U64(n)    => n.to_string(),
                        Value::I64(n)    => n.to_string(),
                        Value::Bool(b)   => b.to_string(),
                        _                => String::new(),
                    })
                    .unwrap_or_default()
            };

            match *entity_type {
                // ── Pump.fun ───────────────────────────────────────────────
                "Buy" => {
                    let sol = get("sol_cost").parse::<u64>().unwrap_or(0);
                    println!(
                        "[slot {:>12}] [{step}] 🟢 Buy         mint={}…  user={}…  tokens={:>14}  sol={:.4}",
                        cs.slot, &get("mint")[..8], &get("user")[..8],
                        get("token_amount"), sol as f64 / 1_000_000_000.0,
                    );
                }
                "Sell" => {
                    let sol = get("sol_output").parse::<u64>().unwrap_or(0);
                    println!(
                        "[slot {:>12}] [{step}] 🔴 Sell        mint={}…  user={}…  tokens={:>14}  sol={:.4}",
                        cs.slot, &get("mint")[..8], &get("user")[..8],
                        get("token_amount"), sol as f64 / 1_000_000_000.0,
                    );
                }
                "Create" => {
                    println!(
                        "[slot {:>12}] [{step}] ✨ Create      mint={}…  name={:12}  sym={}  creator={}…",
                        cs.slot, &get("mint")[..8], get("name"), get("symbol"), &get("creator")[..8],
                    );
                }
                // ── Raydium CLMM ───────────────────────────────────────────
                "RaydiumSwap" => {
                    let amt = get("amount").parse::<u64>().unwrap_or(0);
                    println!(
                        "[slot {:>12}] [{step}] 🔵 RaySwap     pool={}…  user={}…  amount={:>14}  base_in={}",
                        cs.slot, &get("pool")[..8], &get("user")[..8],
                        amt, get("is_base_input"),
                    );
                }
                "RaydiumPosition" => {
                    println!(
                        "[slot {:>12}] [{step}] 🟣 OpenPos     pool={}…  owner={}…  ticks=[{},{}]",
                        cs.slot, &get("pool")[..8], &get("owner")[..8],
                        get("tick_lower"), get("tick_upper"),
                    );
                }
                "RaydiumAddLiquidity" => {
                    println!(
                        "[slot {:>12}] [{step}] ➕ AddLiq      pool={}…  owner={}…  liq={}",
                        cs.slot, &get("pool")[..8], &get("owner")[..8], get("liquidity"),
                    );
                }
                "RaydiumRemoveLiquidity" => {
                    println!(
                        "[slot {:>12}] [{step}] ➖ RemoveLiq   pool={}…  owner={}…  liq={}",
                        cs.slot, &get("pool")[..8], &get("owner")[..8], get("liquidity"),
                    );
                }
                // ── Jupiter v6 ─────────────────────────────────────────────
                "JupiterSwap" => {
                    let src  = get("source_mint");
                    let dst  = get("destination_mint");
                    let src_str  = if src.len() >= 8 { &src[..8] } else { &src };
                    let dst_str  = if dst.len() >= 8 { &dst[..8] } else { &dst };
                    println!(
                        "[slot {:>12}] [{step}] 🟡 JupSwap     user={}…  {src_str}…→{dst_str}…  in={:>12}  hops={}",
                        cs.slot,
                        &get("user")[..8],
                        get("in_amount"),
                        get("hops"),
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
    programs: Vec<String>,
}

fn yellowstone_substrate(cli: &Cli) -> Result<YellowstoneSubstrate> {
    let endpoint = std::env::var("YELLOWSTONE_ENDPOINT")
        .context("YELLOWSTONE_ENDPOINT not set — use --mock for local dev")?;
    let token = std::env::var("YELLOWSTONE_TOKEN").ok();

    let programs = if cli.all {
        vec![
            PUMPFUN_PROGRAM_ID.to_string(),
            RAYDIUM_CLMM_PROGRAM_ID.to_string(),
            JUPITER_V6_PROGRAM_ID.to_string(),
        ]
    } else if cli.raydium {
        vec![RAYDIUM_CLMM_PROGRAM_ID.to_string()]
    } else if cli.jupiter {
        vec![JUPITER_V6_PROGRAM_ID.to_string()]
    } else {
        vec![PUMPFUN_PROGRAM_ID.to_string()]
    };

    Ok(YellowstoneSubstrate { endpoint, token, programs })
}

impl Substrate for YellowstoneSubstrate {
    fn stream(
        &self,
        from: Option<Cursor>,
    ) -> impl futures::Stream<Item = Result<SubstrateEvent>> + Send + '_ {
        // Yellowstone v2 proto has no from_slot field — we filter locally to skip
        // any slots already written before a crash/reconnect.
        let from_slot: Option<u64> = from
            .as_ref()
            .and_then(|c| c.0.as_slice().try_into().ok().map(u64::from_le_bytes));

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
            filters.insert("programs".to_string(), SubscribeRequestFilterTransactions {
                account_include: self.programs.clone(),
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

            tracing::info!(programs = ?self.programs, from_slot = ?from_slot, "Yellowstone stream open");

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

                // Skip slots already processed on a previous run.
                if from_slot.map_or(false, |min| slot <= min) {
                    continue;
                }

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

use std::time::Duration;

use async_stream::stream;
use rand::{Rng, SeedableRng, rngs::SmallRng};
use seahorn_core::{Cursor, RawInstruction, Step, Substrate, SubstrateEvent};
use sha2::{Digest, Sha256};

pub const PUMPFUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

// A handful of "hot" mints so buys/sells cluster on the same tokens
const HOT_MINTS: &[&str] = &[
    "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263",
    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    "So11111111111111111111111111111111111111112",
    "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So",
    "7dHbWXmci3dT8UFYWYZweBLXgycu7Y3iL6trKn1Y7ARj",
];

fn anchor_disc(name: &str) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update(format!("global:{name}"));
    h.finalize()[..8].try_into().unwrap()
}

/// Mock substrate that generates realistic Pump.fun buy/sell/create events.
pub struct PumpfunMockSubstrate {
    pub interval: Duration,
}

impl Default for PumpfunMockSubstrate {
    fn default() -> Self {
        Self { interval: Duration::from_millis(150) }
    }
}

impl Substrate for PumpfunMockSubstrate {
    fn stream(
        &self,
        _from: Option<Cursor>,
    ) -> impl futures::Stream<Item = anyhow::Result<SubstrateEvent>> + Send + '_ {
        let interval = self.interval;
        let buy_disc = anchor_disc("buy");
        let sell_disc = anchor_disc("sell");
        let create_disc = anchor_disc("create");
        let program_id = bs58::decode(PUMPFUN_PROGRAM_ID).into_vec().unwrap();

        stream! {
            let mut rng = SmallRng::from_entropy();
            let mut slot: u64 = 320_000_000;

            // Pre-decode hot mint pubkeys
            let hot_mints: Vec<Vec<u8>> = HOT_MINTS.iter()
                .map(|m| bs58::decode(m).into_vec().unwrap())
                .collect();

            loop {
                slot += rng.gen_range(1..=4u64);

                let roll: f32 = rng.r#gen();
                let ix = if roll < 0.65 {
                    mock_buy(&mut rng, &buy_disc, &program_id, &hot_mints)
                } else if roll < 0.90 {
                    mock_sell(&mut rng, &sell_disc, &program_id, &hot_mints)
                } else {
                    mock_create(&mut rng, &create_disc, &program_id)
                };

                yield Ok(SubstrateEvent {
                    slot,
                    signature: random_bytes(&mut rng, 64),
                    step: Step::New,
                    cursor: Cursor(slot.to_le_bytes().to_vec()),
                    instructions: vec![ix],
                });

                tokio::time::sleep(interval).await;
            }
        }
    }
}

// ── Instruction builders ──────────────────────────────────────────────────────

fn mock_buy(rng: &mut SmallRng, disc: &[u8; 8], program_id: &[u8], mints: &[Vec<u8>]) -> RawInstruction {
    let token_amount: u64 = rng.gen_range(1_000_000..50_000_000_000u64);
    let max_sol_cost: u64 = rng.gen_range(10_000_000..2_000_000_000u64); // 0.01–2 SOL

    let mut data = disc.to_vec();
    data.extend_from_slice(&token_amount.to_le_bytes());
    data.extend_from_slice(&max_sol_cost.to_le_bytes());

    // 12 accounts: global, fee_recipient, mint, bonding_curve, assoc_bc, assoc_user, user, ...
    let mint = mints[rng.gen_range(0..mints.len())].clone();
    let accounts = build_buy_sell_accounts(rng, mint);

    RawInstruction { program_id: program_id.to_vec(), data, accounts }
}

fn mock_sell(rng: &mut SmallRng, disc: &[u8; 8], program_id: &[u8], mints: &[Vec<u8>]) -> RawInstruction {
    let token_amount: u64 = rng.gen_range(500_000..20_000_000_000u64);
    let min_sol_output: u64 = rng.gen_range(1_000_000..500_000_000u64);

    let mut data = disc.to_vec();
    data.extend_from_slice(&token_amount.to_le_bytes());
    data.extend_from_slice(&min_sol_output.to_le_bytes());

    let mint = mints[rng.gen_range(0..mints.len())].clone();
    let accounts = build_buy_sell_accounts(rng, mint);

    RawInstruction { program_id: program_id.to_vec(), data, accounts }
}

fn mock_create(rng: &mut SmallRng, disc: &[u8; 8], program_id: &[u8]) -> RawInstruction {
    let names = ["PumpMoon", "DegenApe", "SolDoge", "RocketCat", "NightOwl", "ChadToken"];
    let syms  = ["PMOON",    "DAPE",     "SDOGE",   "RCAT",      "NOWL",     "CHAD"];
    let idx = rng.gen_range(0..names.len());

    let mut data = disc.to_vec();
    encode_string(&mut data, names[idx]);
    encode_string(&mut data, syms[idx]);
    encode_string(&mut data, &format!("https://pump.fun/metadata/{}.json", syms[idx].to_lowercase()));

    // 14 accounts: mint at 0, creator at 7
    let mut accounts: Vec<Vec<u8>> = (0..14).map(|_| random_bytes(rng, 32)).collect();
    accounts[0] = random_bytes(rng, 32); // fresh mint
    // creator at index 7 stays random

    RawInstruction { program_id: program_id.to_vec(), data, accounts }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build the 12-account layout for buy/sell: mint at idx 2, user at idx 6.
fn build_buy_sell_accounts(rng: &mut SmallRng, mint: Vec<u8>) -> Vec<Vec<u8>> {
    let mut accounts: Vec<Vec<u8>> = (0..12).map(|_| random_bytes(rng, 32)).collect();
    accounts[2] = mint;
    accounts[6] = random_bytes(rng, 32); // user wallet
    accounts
}

/// Borsh string encoding: u32 LE length prefix + UTF-8 bytes.
fn encode_string(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(bytes);
}

fn random_bytes(rng: &mut SmallRng, len: usize) -> Vec<u8> {
    (0..len).map(|_| rng.r#gen::<u8>()).collect()
}

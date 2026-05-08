use std::time::Duration;

use async_stream::stream;
use rand::{Rng, SeedableRng, rngs::SmallRng};
use seahorn_core::{Cursor, RawInstruction, Step, Substrate, SubstrateEvent};
use sha2::{Digest, Sha256};

pub const JUPITER_V6_PROGRAM_ID: &str = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";

// Common Solana token mints for realistic pairs
const MINTS: &[&str] = &[
    "So11111111111111111111111111111111111111112",  // SOL (wSOL)
    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", // USDC
    "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",  // USDT
    "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So",  // mSOL
    "7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs",  // ETH (Wormhole)
    "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263",  // BONK
];

fn anchor_disc(name: &str) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update(format!("global:{name}"));
    h.finalize()[..8].try_into().unwrap()
}

/// Mock substrate that generates realistic Jupiter v6 aggregated swap events.
pub struct JupiterV6MockSubstrate {
    pub interval: Duration,
}

impl Default for JupiterV6MockSubstrate {
    fn default() -> Self {
        Self { interval: Duration::from_millis(100) }
    }
}

impl Substrate for JupiterV6MockSubstrate {
    fn stream(
        &self,
        _from: Option<Cursor>,
    ) -> impl futures::Stream<Item = anyhow::Result<SubstrateEvent>> + Send + '_ {
        let interval = self.interval;
        let shared_route_disc = anchor_disc("shared_accounts_route");
        let shared_exact_disc = anchor_disc("shared_accounts_exact_out_route");
        let program_id        = bs58::decode(JUPITER_V6_PROGRAM_ID).into_vec().unwrap();

        stream! {
            let mut rng  = SmallRng::from_entropy();
            let mut slot: u64 = 320_000_000;

            let mints: Vec<Vec<u8>> = MINTS.iter()
                .map(|m| bs58::decode(m).into_vec().unwrap())
                .collect();

            loop {
                slot += rng.gen_range(1..=3u64);

                let exact_out = rng.gen_bool(0.15);
                let disc = if exact_out { &shared_exact_disc } else { &shared_route_disc };

                let ix = mock_shared_route(&mut rng, disc, &program_id, &mints, exact_out);

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

// ── Instruction builder ────────────────────────────────────────────────────────

fn mock_shared_route(
    rng: &mut SmallRng,
    disc: &[u8; 8],
    program_id: &[u8],
    mints: &[Vec<u8>],
    _exact_out: bool,
) -> RawInstruction {
    let hops: u8 = rng.gen_range(1..=3u8);

    let mut data = disc.to_vec();

    // id: u8 (route bundle id)
    data.push(rng.r#gen::<u8>());

    // route_plan: Vec<RoutePlanStep>
    // Each step: swap_discriminant(u8=7 Raydium, no fields) + percent(u8) + in_idx(u8) + out_idx(u8)
    data.extend_from_slice(&(hops as u32).to_le_bytes()); // vec length
    for i in 0..hops {
        data.push(7u8); // Swap::Raydium — no extra fields
        let percent = if i == hops - 1 { 100u8 } else { rng.gen_range(10..90u8) };
        data.push(percent);
        data.push(i);       // input_index
        data.push(i + 1);   // output_index
    }

    // in_amount / out_amount
    let amount_a: u64 = rng.gen_range(100_000..10_000_000_000u64);
    let amount_b: u64 = rng.gen_range(90_000..9_900_000_000u64);
    data.extend_from_slice(&amount_a.to_le_bytes()); // in_amount (or out_amount for exact_out)
    data.extend_from_slice(&amount_b.to_le_bytes()); // quoted_out_amount (or quoted_in_amount)

    let slippage_bps: u16 = rng.gen_range(1..=100u16);
    data.extend_from_slice(&slippage_bps.to_le_bytes());
    data.push(0u8); // platform_fee_bps

    // 12 accounts for shared_accounts_route:
    //   [0] token_program  [1] program_authority  [2] user_transfer_authority
    //   [3] source_token_acct  [4] prog_src_acct  [5] prog_dst_acct
    //   [6] dst_token_acct  [7] source_mint  [8] destination_mint  ...
    let src_idx  = rng.gen_range(0..mints.len());
    let mut dst_idx = rng.gen_range(0..mints.len());
    if dst_idx == src_idx { dst_idx = (src_idx + 1) % mints.len(); }

    let mut accounts: Vec<Vec<u8>> = (0..12).map(|_| random_bytes(rng, 32)).collect();
    accounts[2] = random_bytes(rng, 32); // user wallet
    accounts[7] = mints[src_idx].clone();
    accounts[8] = mints[dst_idx].clone();

    RawInstruction { program_id: program_id.to_vec(), data, accounts }
}

fn random_bytes(rng: &mut SmallRng, len: usize) -> Vec<u8> {
    (0..len).map(|_| rng.r#gen::<u8>()).collect()
}

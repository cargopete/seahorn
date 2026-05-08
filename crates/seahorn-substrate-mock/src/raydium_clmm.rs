use std::time::Duration;

use async_stream::stream;
use rand::{Rng, SeedableRng, rngs::SmallRng};
use seahorn_core::{Cursor, RawInstruction, Step, Substrate, SubstrateEvent};
use sha2::{Digest, Sha256};

pub const RAYDIUM_CLMM_PROGRAM_ID: &str = "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK";

// A handful of preset CLMM pools (SOL/USDC, SOL/USDT, RAY/SOL, mSOL/SOL, etc.)
const HOT_POOLS: &[&str] = &[
    "8sLbNZoA1cfnvMJLPfp98ZLAnFSYCFApfJKMbiXNLwxj",
    "2QdhepnKRTLjjSqPL1PtKNwqrUkoLee5Gqs8bvZhRdAv",
    "61acRgpURKTU8LKPJKs6WQa18KzD9ogavXzjxfD84KLu",
    "AiMZS5U3JMvpdvsr1KeaMiS354Z1bCGZyLJ3jVkrAQW6",
    "FpCMFDFGYotvufJ7HrFHsWEiiQCGbkLCtwHiDnh7o28Q",
];

fn anchor_disc(name: &str) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update(format!("global:{name}"));
    h.finalize()[..8].try_into().unwrap()
}

/// Mock substrate that generates realistic Raydium CLMM events.
pub struct RaydiumClmmMockSubstrate {
    pub interval: Duration,
}

impl Default for RaydiumClmmMockSubstrate {
    fn default() -> Self {
        Self { interval: Duration::from_millis(150) }
    }
}

impl Substrate for RaydiumClmmMockSubstrate {
    fn stream(
        &self,
        _from: Option<Cursor>,
    ) -> impl futures::Stream<Item = anyhow::Result<SubstrateEvent>> + Send + '_ {
        let interval = self.interval;

        let swap_disc      = anchor_disc("swap");
        let open_disc      = anchor_disc("open_position");
        let increase_disc  = anchor_disc("increase_liquidity");
        let decrease_disc  = anchor_disc("decrease_liquidity");
        let program_id     = bs58::decode(RAYDIUM_CLMM_PROGRAM_ID).into_vec().unwrap();

        stream! {
            let mut rng  = SmallRng::from_entropy();
            let mut slot: u64 = 320_000_000;

            let hot_pools: Vec<Vec<u8>> = HOT_POOLS.iter()
                .map(|p| bs58::decode(p).into_vec().unwrap())
                .collect();

            loop {
                slot += rng.gen_range(1..=4u64);

                let roll: f32 = rng.r#gen();
                let ix = if roll < 0.75 {
                    mock_swap(&mut rng, &swap_disc, &program_id, &hot_pools)
                } else if roll < 0.90 {
                    mock_open_position(&mut rng, &open_disc, &program_id, &hot_pools)
                } else if roll < 0.95 {
                    mock_change_liquidity(&mut rng, &increase_disc, &program_id, &hot_pools)
                } else {
                    mock_change_liquidity(&mut rng, &decrease_disc, &program_id, &hot_pools)
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

// ── Instruction builders ───────────────────────────────────────────────────────

fn mock_swap(rng: &mut SmallRng, disc: &[u8; 8], program_id: &[u8], pools: &[Vec<u8>]) -> RawInstruction {
    let amount: u64           = rng.gen_range(1_000_000..10_000_000_000u64);
    let other_threshold: u64  = rng.gen_range(900_000..9_000_000_000u64);
    let sqrt_price_limit: u128 = rng.gen_range(0u128..u64::MAX as u128);
    let is_base_input: bool    = rng.r#gen::<bool>();

    let mut data = disc.to_vec();
    data.extend_from_slice(&amount.to_le_bytes());
    data.extend_from_slice(&other_threshold.to_le_bytes());
    data.extend_from_slice(&sqrt_price_limit.to_le_bytes());
    data.push(is_base_input as u8);

    // 14 accounts: pool @ 2, user/payer @ 0
    let pool = pools[rng.gen_range(0..pools.len())].clone();
    let mut accounts: Vec<Vec<u8>> = (0..14).map(|_| random_bytes(rng, 32)).collect();
    accounts[2] = pool;

    RawInstruction { program_id: program_id.to_vec(), data, accounts }
}

fn mock_open_position(rng: &mut SmallRng, disc: &[u8; 8], program_id: &[u8], pools: &[Vec<u8>]) -> RawInstruction {
    let tick_lower: i32 = -(rng.gen_range(100..=2000i32));
    let tick_upper: i32 = rng.gen_range(100..=2000i32);
    let tick_arr_lower_start: i32 = tick_lower - (tick_lower % 64);
    let tick_arr_upper_start: i32 = tick_upper - (tick_upper % 64);
    let liquidity: u128   = rng.gen_range(1_000_000_000u128..1_000_000_000_000u128);
    let amount_0_max: u64 = rng.gen_range(1_000_000..1_000_000_000u64);
    let amount_1_max: u64 = rng.gen_range(1_000_000..1_000_000_000u64);

    let mut data = disc.to_vec();
    data.extend_from_slice(&tick_lower.to_le_bytes());
    data.extend_from_slice(&tick_upper.to_le_bytes());
    data.extend_from_slice(&tick_arr_lower_start.to_le_bytes());
    data.extend_from_slice(&tick_arr_upper_start.to_le_bytes());
    data.extend_from_slice(&liquidity.to_le_bytes());
    data.extend_from_slice(&amount_0_max.to_le_bytes());
    data.extend_from_slice(&amount_1_max.to_le_bytes());

    // 18 accounts: pool @ 5, nft_owner @ 1
    let pool = pools[rng.gen_range(0..pools.len())].clone();
    let mut accounts: Vec<Vec<u8>> = (0..18).map(|_| random_bytes(rng, 32)).collect();
    accounts[5] = pool;

    RawInstruction { program_id: program_id.to_vec(), data, accounts }
}

fn mock_change_liquidity(rng: &mut SmallRng, disc: &[u8; 8], program_id: &[u8], pools: &[Vec<u8>]) -> RawInstruction {
    let liquidity: u128 = rng.gen_range(100_000_000u128..500_000_000_000u128);
    let amount_0: u64   = rng.gen_range(100_000..500_000_000u64);
    let amount_1: u64   = rng.gen_range(100_000..500_000_000u64);

    let mut data = disc.to_vec();
    data.extend_from_slice(&liquidity.to_le_bytes());
    data.extend_from_slice(&amount_0.to_le_bytes());
    data.extend_from_slice(&amount_1.to_le_bytes());

    // 13 accounts: pool @ 2, owner @ 0
    let pool = pools[rng.gen_range(0..pools.len())].clone();
    let mut accounts: Vec<Vec<u8>> = (0..13).map(|_| random_bytes(rng, 32)).collect();
    accounts[2] = pool;

    RawInstruction { program_id: program_id.to_vec(), data, accounts }
}

fn random_bytes(rng: &mut SmallRng, len: usize) -> Vec<u8> {
    (0..len).map(|_| rng.r#gen::<u8>()).collect()
}

use std::time::Duration;

use async_stream::stream;
use rand::{Rng, SeedableRng, rngs::SmallRng};
use seahorn_core::{Cursor, RawInstruction, Step, Substrate, SubstrateEvent};

// Raydium AMM v4 program ID (as bytes)
const RAYDIUM_AMM_V4: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";

const SWAP_BASE_IN: u8 = 9;
const SWAP_BASE_OUT: u8 = 11;

/// A mock substrate that generates synthetic Raydium swap transactions.
///
/// Useful for local development without a Yellowstone endpoint. Produces
/// realistic slot numbers, base58 signatures, and instruction layouts.
pub struct MockSubstrate {
    /// Average delay between emitted events.
    pub interval: Duration,
}

impl Default for MockSubstrate {
    fn default() -> Self {
        Self { interval: Duration::from_millis(200) }
    }
}

impl Substrate for MockSubstrate {
    fn stream(
        &self,
        _from: Option<Cursor>,
    ) -> impl futures::Stream<Item = anyhow::Result<SubstrateEvent>> + Send + '_ {
        let interval = self.interval;

        stream! {
            let mut rng = SmallRng::from_entropy();
            let mut slot: u64 = 320_000_000;

            loop {
                slot += rng.gen_range(1..=3u64);

                let discriminator = if rng.gen_bool(0.7) { SWAP_BASE_IN } else { SWAP_BASE_OUT };
                let amount_in: u64 = rng.gen_range(1_000_000..500_000_000_000u64);
                let min_out: u64 = rng.gen_range(100_000..400_000_000_000u64);

                // Build a synthetic instruction matching Raydium SwapBaseIn/Out layout:
                // [discriminator: u8, amount_in: u64 LE, min_amount_out: u64 LE]
                let mut data = vec![discriminator];
                data.extend_from_slice(&amount_in.to_le_bytes());
                data.extend_from_slice(&min_out.to_le_bytes());

                let program_id = bs58::decode(RAYDIUM_AMM_V4).into_vec().unwrap();

                let event = SubstrateEvent {
                    slot,
                    signature: random_bytes(&mut rng, 64),
                    step: Step::New,
                    cursor: Cursor(slot.to_le_bytes().to_vec()),
                    instructions: vec![RawInstruction {
                        program_id,
                        data,
                        accounts: (0..18).map(|_| random_bytes(&mut rng, 32)).collect(),
                    }],
                };

                yield Ok(event);
                tokio::time::sleep(interval).await;
            }
        }
    }
}

fn random_bytes(rng: &mut SmallRng, len: usize) -> Vec<u8> {
    (0..len).map(|_| rng.r#gen::<u8>()).collect()
}

pub mod pumpfun;
pub use pumpfun::PumpfunMockSubstrate;

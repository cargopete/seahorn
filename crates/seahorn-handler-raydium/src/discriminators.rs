use sha2::{Digest, Sha256};
use std::sync::OnceLock;

fn anchor_disc(name: &str) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update(format!("global:{name}"));
    h.finalize()[..8].try_into().unwrap()
}

static SWAP:               OnceLock<[u8; 8]> = OnceLock::new();
static SWAP_V2:            OnceLock<[u8; 8]> = OnceLock::new();
static OPEN_POSITION:      OnceLock<[u8; 8]> = OnceLock::new();
static INCREASE_LIQUIDITY: OnceLock<[u8; 8]> = OnceLock::new();
static INCREASE_LIQ_V2:    OnceLock<[u8; 8]> = OnceLock::new();
static DECREASE_LIQUIDITY: OnceLock<[u8; 8]> = OnceLock::new();
static DECREASE_LIQ_V2:    OnceLock<[u8; 8]> = OnceLock::new();

pub fn swap()               -> &'static [u8; 8] { SWAP.get_or_init(|| anchor_disc("swap")) }
pub fn swap_v2()            -> &'static [u8; 8] { SWAP_V2.get_or_init(|| anchor_disc("swap_v2")) }
pub fn open_position()      -> &'static [u8; 8] { OPEN_POSITION.get_or_init(|| anchor_disc("open_position")) }
pub fn increase_liquidity() -> &'static [u8; 8] { INCREASE_LIQUIDITY.get_or_init(|| anchor_disc("increase_liquidity")) }
pub fn increase_liq_v2()    -> &'static [u8; 8] { INCREASE_LIQ_V2.get_or_init(|| anchor_disc("increase_liquidity_v2")) }
pub fn decrease_liquidity() -> &'static [u8; 8] { DECREASE_LIQUIDITY.get_or_init(|| anchor_disc("decrease_liquidity")) }
pub fn decrease_liq_v2()    -> &'static [u8; 8] { DECREASE_LIQ_V2.get_or_init(|| anchor_disc("decrease_liquidity_v2")) }

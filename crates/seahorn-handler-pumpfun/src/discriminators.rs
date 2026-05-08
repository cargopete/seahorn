use sha2::{Digest, Sha256};
use std::sync::OnceLock;

/// Compute an Anchor instruction discriminator: sha256("global:{name}")[..8]
fn anchor_disc(name: &str) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update(format!("global:{name}"));
    h.finalize()[..8].try_into().unwrap()
}

static BUY: OnceLock<[u8; 8]> = OnceLock::new();
static SELL: OnceLock<[u8; 8]> = OnceLock::new();
static CREATE: OnceLock<[u8; 8]> = OnceLock::new();

pub fn buy() -> &'static [u8; 8] { BUY.get_or_init(|| anchor_disc("buy")) }
pub fn sell() -> &'static [u8; 8] { SELL.get_or_init(|| anchor_disc("sell")) }
pub fn create() -> &'static [u8; 8] { CREATE.get_or_init(|| anchor_disc("create")) }

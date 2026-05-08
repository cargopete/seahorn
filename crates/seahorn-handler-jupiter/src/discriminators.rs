use sha2::{Digest, Sha256};
use std::sync::OnceLock;

fn anchor_disc(name: &str) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update(format!("global:{name}"));
    h.finalize()[..8].try_into().unwrap()
}

static ROUTE:                           OnceLock<[u8; 8]> = OnceLock::new();
static SHARED_ACCOUNTS_ROUTE:           OnceLock<[u8; 8]> = OnceLock::new();
static EXACT_OUT_ROUTE:                 OnceLock<[u8; 8]> = OnceLock::new();
static SHARED_ACCOUNTS_EXACT_OUT_ROUTE: OnceLock<[u8; 8]> = OnceLock::new();
static ROUTE_WITH_TOKEN_LEDGER:         OnceLock<[u8; 8]> = OnceLock::new();

pub fn route()                           -> &'static [u8; 8] { ROUTE.get_or_init(|| anchor_disc("route")) }
pub fn shared_accounts_route()           -> &'static [u8; 8] { SHARED_ACCOUNTS_ROUTE.get_or_init(|| anchor_disc("shared_accounts_route")) }
pub fn exact_out_route()                 -> &'static [u8; 8] { EXACT_OUT_ROUTE.get_or_init(|| anchor_disc("exact_out_route")) }
pub fn shared_accounts_exact_out_route() -> &'static [u8; 8] { SHARED_ACCOUNTS_EXACT_OUT_ROUTE.get_or_init(|| anchor_disc("shared_accounts_exact_out_route")) }
pub fn route_with_token_ledger()         -> &'static [u8; 8] { ROUTE_WITH_TOKEN_LEDGER.get_or_init(|| anchor_disc("route_with_token_ledger")) }

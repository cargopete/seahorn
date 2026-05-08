pub const RAYDIUM_CLMM_PROGRAM_ID: &str = "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK";

/// Pre-decoded program id bytes, compared directly against RawInstruction::program_id.
pub static RAYDIUM_CLMM_PROGRAM_ID_BYTES: std::sync::LazyLock<Vec<u8>> =
    std::sync::LazyLock::new(|| bs58::decode(RAYDIUM_CLMM_PROGRAM_ID).into_vec().unwrap());

mod decode;
mod discriminators;
mod handler;

pub use handler::RaydiumClmmHandler;

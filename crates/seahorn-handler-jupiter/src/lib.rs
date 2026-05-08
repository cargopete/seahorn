pub const JUPITER_V6_PROGRAM_ID: &str = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";

pub static JUPITER_V6_PROGRAM_ID_BYTES: std::sync::LazyLock<Vec<u8>> =
    std::sync::LazyLock::new(|| bs58::decode(JUPITER_V6_PROGRAM_ID).into_vec().unwrap());

mod decode;
mod discriminators;
mod handler;

pub use handler::JupiterV6Handler;

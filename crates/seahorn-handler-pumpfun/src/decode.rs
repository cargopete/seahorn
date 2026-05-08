use seahorn_core::RawInstruction;
use crate::discriminators;

// Account indices in Pump.fun buy/sell instructions (from the Pump.fun IDL)
const BUY_SELL_MINT_IDX: usize = 2;
const BUY_SELL_USER_IDX: usize = 6;

// Account indices in Pump.fun create instruction
const CREATE_MINT_IDX: usize = 0;
const CREATE_USER_IDX: usize = 7;

#[derive(Debug, Clone)]
pub struct BuyInstruction {
    pub mint: String,
    pub user: String,
    /// Raw token amount (u64, no decimals)
    pub token_amount: u64,
    /// Maximum SOL to spend, in lamports
    pub max_sol_cost: u64,
}

#[derive(Debug, Clone)]
pub struct SellInstruction {
    pub mint: String,
    pub user: String,
    /// Raw token amount (u64, no decimals)
    pub token_amount: u64,
    /// Minimum SOL to receive, in lamports
    pub min_sol_output: u64,
}

#[derive(Debug, Clone)]
pub struct CreateInstruction {
    pub mint: String,
    pub creator: String,
    pub name: String,
    pub symbol: String,
    pub uri: String,
}

#[derive(Debug, Clone)]
pub enum PumpfunInstruction {
    Buy(BuyInstruction),
    Sell(SellInstruction),
    Create(CreateInstruction),
}

/// Attempt to decode a raw instruction as a Pump.fun instruction.
/// Returns `None` if the discriminator doesn't match or data is malformed.
pub fn decode(ix: &RawInstruction) -> Option<PumpfunInstruction> {
    if ix.data.len() < 8 {
        return None;
    }

    let disc = &ix.data[..8];
    let args = &ix.data[8..];

    if disc == discriminators::buy().as_slice() {
        decode_buy(ix, args)
    } else if disc == discriminators::sell().as_slice() {
        decode_sell(ix, args)
    } else if disc == discriminators::create().as_slice() {
        decode_create(ix, args)
    } else {
        None
    }
}

fn decode_buy(ix: &RawInstruction, args: &[u8]) -> Option<PumpfunInstruction> {
    let mut cur = 0;
    let token_amount = read_u64(args, &mut cur)?;
    let max_sol_cost = read_u64(args, &mut cur)?;
    Some(PumpfunInstruction::Buy(BuyInstruction {
        mint: pubkey(ix.accounts.get(BUY_SELL_MINT_IDX)?),
        user: pubkey(ix.accounts.get(BUY_SELL_USER_IDX)?),
        token_amount,
        max_sol_cost,
    }))
}

fn decode_sell(ix: &RawInstruction, args: &[u8]) -> Option<PumpfunInstruction> {
    let mut cur = 0;
    let token_amount = read_u64(args, &mut cur)?;
    let min_sol_output = read_u64(args, &mut cur)?;
    Some(PumpfunInstruction::Sell(SellInstruction {
        mint: pubkey(ix.accounts.get(BUY_SELL_MINT_IDX)?),
        user: pubkey(ix.accounts.get(BUY_SELL_USER_IDX)?),
        token_amount,
        min_sol_output,
    }))
}

fn decode_create(ix: &RawInstruction, args: &[u8]) -> Option<PumpfunInstruction> {
    let mut cur = 0;
    let name = read_string(args, &mut cur)?;
    let symbol = read_string(args, &mut cur)?;
    let uri = read_string(args, &mut cur)?;
    Some(PumpfunInstruction::Create(CreateInstruction {
        mint: pubkey(ix.accounts.get(CREATE_MINT_IDX)?),
        creator: pubkey(ix.accounts.get(CREATE_USER_IDX)?),
        name,
        symbol,
        uri,
    }))
}

// ── Borsh primitives ──────────────────────────────────────────────────────────

fn read_u64(data: &[u8], cur: &mut usize) -> Option<u64> {
    let end = *cur + 8;
    let v = u64::from_le_bytes(data.get(*cur..end)?.try_into().ok()?);
    *cur = end;
    Some(v)
}

fn read_string(data: &[u8], cur: &mut usize) -> Option<String> {
    let len = read_u64(data, cur)? as usize; // Borsh strings: u32 LE length prefix
    let end = *cur + len;
    let s = String::from_utf8(data.get(*cur..end)?.to_vec()).ok()?;
    *cur = end;
    Some(s)
}

fn pubkey(bytes: &[u8]) -> String {
    bs58::encode(bytes).into_string()
}

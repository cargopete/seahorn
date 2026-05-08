/// Raydium CLMM instruction data decoded from Borsh encoding.
///
/// Account index layout (from Raydium CLMM IDL):
///   swap / swap_v2:          pool @ 2, user/payer @ 0
///   open_position:            pool @ 5, nft_owner @ 1
///   increase/decrease_liq:   pool @ 2, nft_owner @ 0

pub struct SwapInstruction {
    pub pool:            Vec<u8>,
    pub user:            Vec<u8>,
    pub amount:          u64,
    pub other_threshold: u64,
    // sqrt_price_limit_x64 is u128 — stored as string in Value
    pub sqrt_price_limit: u128,
    pub is_base_input:   bool,
}

pub struct OpenPositionInstruction {
    pub pool:        Vec<u8>,
    pub owner:       Vec<u8>,
    pub tick_lower:  i32,
    pub tick_upper:  i32,
    pub liquidity:   u128,
    pub amount_0_max: u64,
    pub amount_1_max: u64,
}

pub struct ChangeLiquidityInstruction {
    pub pool:      Vec<u8>,
    pub owner:     Vec<u8>,
    pub liquidity: u128,
    pub amount_0:  u64,
    pub amount_1:  u64,
}

// ── Decoders ──────────────────────────────────────────────────────────────────

pub fn decode_swap(data: &[u8], accounts: &[Vec<u8>]) -> Option<SwapInstruction> {
    // data: [disc(8)] amount(8) other_threshold(8) sqrt_price_limit(16) is_base_input(1)
    let mut off = 8usize;
    let amount          = read_u64(data, &mut off)?;
    let other_threshold = read_u64(data, &mut off)?;
    let sqrt_price_limit = read_u128(data, &mut off)?;
    let is_base_input   = read_bool(data, &mut off)?;

    Some(SwapInstruction {
        pool:  accounts.get(2)?.clone(),
        user:  accounts.get(0)?.clone(),
        amount,
        other_threshold,
        sqrt_price_limit,
        is_base_input,
    })
}

pub fn decode_open_position(data: &[u8], accounts: &[Vec<u8>]) -> Option<OpenPositionInstruction> {
    // data: [disc(8)] tick_lower(4) tick_upper(4) tick_arr_lower_start(4) tick_arr_upper_start(4)
    //       liquidity(16) amount_0_max(8) amount_1_max(8)
    let mut off = 8usize;
    let tick_lower  = read_i32(data, &mut off)?;
    let tick_upper  = read_i32(data, &mut off)?;
    let _tick_arr_lower = read_i32(data, &mut off)?;
    let _tick_arr_upper = read_i32(data, &mut off)?;
    let liquidity   = read_u128(data, &mut off)?;
    let amount_0_max = read_u64(data, &mut off)?;
    let amount_1_max = read_u64(data, &mut off)?;

    Some(OpenPositionInstruction {
        pool:  accounts.get(5)?.clone(),
        owner: accounts.get(1)?.clone(),
        tick_lower,
        tick_upper,
        liquidity,
        amount_0_max,
        amount_1_max,
    })
}

pub fn decode_change_liquidity(data: &[u8], accounts: &[Vec<u8>]) -> Option<ChangeLiquidityInstruction> {
    // data: [disc(8)] liquidity(16) amount_0(8) amount_1(8)
    let mut off = 8usize;
    let liquidity = read_u128(data, &mut off)?;
    let amount_0  = read_u64(data, &mut off)?;
    let amount_1  = read_u64(data, &mut off)?;

    Some(ChangeLiquidityInstruction {
        pool:  accounts.get(2)?.clone(),
        owner: accounts.get(0)?.clone(),
        liquidity,
        amount_0,
        amount_1,
    })
}

// ── Borsh primitives ──────────────────────────────────────────────────────────

fn read_u64(data: &[u8], off: &mut usize) -> Option<u64> {
    let b: [u8; 8] = data.get(*off..*off + 8)?.try_into().ok()?;
    *off += 8;
    Some(u64::from_le_bytes(b))
}

fn read_u128(data: &[u8], off: &mut usize) -> Option<u128> {
    let b: [u8; 16] = data.get(*off..*off + 16)?.try_into().ok()?;
    *off += 16;
    Some(u128::from_le_bytes(b))
}

fn read_i32(data: &[u8], off: &mut usize) -> Option<i32> {
    let b: [u8; 4] = data.get(*off..*off + 4)?.try_into().ok()?;
    *off += 4;
    Some(i32::from_le_bytes(b))
}

fn read_bool(data: &[u8], off: &mut usize) -> Option<bool> {
    let b = *data.get(*off)?;
    *off += 1;
    Some(b != 0)
}

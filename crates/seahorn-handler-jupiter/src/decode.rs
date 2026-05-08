/// Jupiter v6 instruction decoder.
///
/// The tricky part: instruction data contains `route_plan: Vec<RoutePlanStep>`
/// before the amounts. Each step's `Swap` enum has 40+ variants of different
/// byte widths. We skip each step using a lookup table of extra-field sizes per
/// variant discriminant, then read `in_amount` / `quoted_out_amount`.
///
/// Account index layout (shared_accounts_route — most common):
///   accounts[2] = user_transfer_authority (user)
///   accounts[7] = source_mint (input token)
///   accounts[8] = destination_mint (output token)
///
/// For route / route_with_token_ledger:
///   accounts[1] = user_transfer_authority
///   (mints not at fixed indices — too variable)

pub struct RouteInstruction {
    pub user:               Vec<u8>,
    pub source_mint:        Option<Vec<u8>>,
    pub destination_mint:   Option<Vec<u8>>,
    pub in_amount:          u64,
    pub quoted_out_amount:  u64,
    pub slippage_bps:       u16,
    pub platform_fee_bps:   u8,
    pub hops:               u8,
}

pub struct ExactOutInstruction {
    pub user:              Vec<u8>,
    pub source_mint:       Option<Vec<u8>>,
    pub destination_mint:  Option<Vec<u8>>,
    pub out_amount:        u64,
    pub quoted_in_amount:  u64,
    pub slippage_bps:      u16,
    pub platform_fee_bps:  u8,
    pub hops:              u8,
}

// ── Public decoders ───────────────────────────────────────────────────────────

/// Decode `route` or `route_with_token_ledger`.
pub fn decode_route(data: &[u8], accounts: &[Vec<u8>]) -> Option<RouteInstruction> {
    let mut off = 8usize; // skip discriminator
    let hops = skip_route_plan(data, &mut off)?;
    let in_amount         = read_u64(data, &mut off)?;
    let quoted_out_amount = read_u64(data, &mut off)?;
    let slippage_bps      = read_u16(data, &mut off)?;
    let platform_fee_bps  = read_u8(data, &mut off)?;

    Some(RouteInstruction {
        user:              accounts.get(1)?.clone(),
        source_mint:       None,
        destination_mint:  None,
        in_amount,
        quoted_out_amount,
        slippage_bps,
        platform_fee_bps,
        hops,
    })
}

/// Decode `shared_accounts_route`.
///
/// Has an extra `id: u8` before the route_plan, and exposes mints at fixed
/// account indices — so we can extract source and destination mints reliably.
pub fn decode_shared_route(data: &[u8], accounts: &[Vec<u8>]) -> Option<RouteInstruction> {
    let mut off = 8usize; // skip discriminator
    let _id = read_u8(data, &mut off)?;  // route bundle id
    let hops = skip_route_plan(data, &mut off)?;
    let in_amount         = read_u64(data, &mut off)?;
    let quoted_out_amount = read_u64(data, &mut off)?;
    let slippage_bps      = read_u16(data, &mut off)?;
    let platform_fee_bps  = read_u8(data, &mut off)?;

    Some(RouteInstruction {
        user:             accounts.get(2)?.clone(),
        source_mint:      accounts.get(7).cloned(),
        destination_mint: accounts.get(8).cloned(),
        in_amount,
        quoted_out_amount,
        slippage_bps,
        platform_fee_bps,
        hops,
    })
}

/// Decode `exact_out_route`.
pub fn decode_exact_out(data: &[u8], accounts: &[Vec<u8>]) -> Option<ExactOutInstruction> {
    let mut off = 8usize;
    let hops = skip_route_plan(data, &mut off)?;
    let out_amount        = read_u64(data, &mut off)?;
    let quoted_in_amount  = read_u64(data, &mut off)?;
    let slippage_bps      = read_u16(data, &mut off)?;
    let platform_fee_bps  = read_u8(data, &mut off)?;

    Some(ExactOutInstruction {
        user:             accounts.get(1)?.clone(),
        source_mint:      None,
        destination_mint: None,
        out_amount,
        quoted_in_amount,
        slippage_bps,
        platform_fee_bps,
        hops,
    })
}

/// Decode `shared_accounts_exact_out_route`.
pub fn decode_shared_exact_out(data: &[u8], accounts: &[Vec<u8>]) -> Option<ExactOutInstruction> {
    let mut off = 8usize;
    let _id = read_u8(data, &mut off)?;
    let hops = skip_route_plan(data, &mut off)?;
    let out_amount        = read_u64(data, &mut off)?;
    let quoted_in_amount  = read_u64(data, &mut off)?;
    let slippage_bps      = read_u16(data, &mut off)?;
    let platform_fee_bps  = read_u8(data, &mut off)?;

    Some(ExactOutInstruction {
        user:             accounts.get(2)?.clone(),
        source_mint:      accounts.get(7).cloned(),
        destination_mint: accounts.get(8).cloned(),
        out_amount,
        quoted_in_amount,
        slippage_bps,
        platform_fee_bps,
        hops,
    })
}

// ── Route plan skipping ───────────────────────────────────────────────────────

/// Read the route_plan Vec header and skip every step.
/// Returns the number of hops, or None if data is malformed or contains
/// an unknown Swap variant (new Jupiter version not yet in our table).
fn skip_route_plan(data: &[u8], off: &mut usize) -> Option<u8> {
    let len = read_u32(data, off)? as usize;
    for _ in 0..len {
        skip_route_plan_step(data, off)?;
    }
    Some(len.min(255) as u8)
}

fn skip_route_plan_step(data: &[u8], off: &mut usize) -> Option<()> {
    // Swap enum discriminant (u8)
    let discriminant = read_u8(data, off)?;
    let extra = swap_extra_bytes(discriminant)?;
    if *off + extra > data.len() {
        return None;
    }
    *off += extra;

    // percent: u8, input_index: u8, output_index: u8
    *off += 3;
    if *off > data.len() { return None; }
    Some(())
}

/// Returns the number of extra bytes for a given Swap enum variant's fields.
/// Returns None for unknown discriminants (new Jupiter version).
///
/// Source: Jupiter v6 IDL (routerV6 ~v6.4)
fn swap_extra_bytes(discriminant: u8) -> Option<usize> {
    match discriminant {
        // Field-less variants (just the discriminant byte)
        0  |  // Saber
        1  |  // SaberAddDecimalsDeposit
        2  |  // SaberAddDecimalsWithdraw
        3  |  // TokenSwap
        4  |  // Sencha
        5  |  // Step
        6  |  // Cropper
        7  |  // Raydium
        9  |  // Lifinity
        10 |  // Mercurial
        11 |  // Cykura
        13 |  // MarinadeDeposit
        14 |  // MarinadeUnstake
        19 |  // Meteora
        20 |  // GooseFx
        22 |  // Balansol
        25 |  // LifinityV2
        26 |  // RaydiumClmm
        30 |  // TokenSwapV2
        31 |  // HeliumTreasuryManagementRedeemV0
        32 |  // StakeDexStakeWrappedSol
        34 |  // GooseFxV2
        35 |  // Perps
        36 |  // PerpsAddLiquidity
        37 |  // PerpsRemoveLiquidity
        38 |  // MeteoraDlmm
        40 |  // RaydiumClmmV2
        41 |  // ClaimStake
        42 |  // PoolsOfSol (if present)
        44 |  // OneIntro
        45 |  // PumpdotfunWrappedBuy
        46 |  // PumpdotfunWrappedSell
        47 |  // PerpsV2
        48 |  // PerpsV2AddLiquidity
        49    // PerpsV2RemoveLiquidity
        => Some(0),

        // 1-byte field: bool or u8 enum (e.g. Side)
        8  |  // Crema { a_to_b: bool }
        12 |  // Serum { side: Side }
        15 |  // Aldrin { side: Side }
        16 |  // AldrinV2 { side: Side }
        17 |  // Whirlpool { a_to_b: bool }
        18 |  // InvariantSwap { x_to_y: bool }
        21 |  // DeltaFi { stable_swap: bool }
        23 |  // MarcoPolo { x_to_y: bool }
        24 |  // Dradex { side: Side }
        27 |  // Openbook { side: Side }
        28 |  // Phoenix { side: Side }
        39 |  // OpenbookV2 { side: Side }
        43 |  // Obric { x_to_y: bool }
        50    // StabbleStableSwap / StabbleWeightedSwap / ObricV2
        => Some(1),

        // 4-byte field: u32
        33    // StakeDexSwapViaStake { bridge_stake_seed: u32 }
        => Some(4),

        // 16-byte field: two u64
        29    // Symmetry { from_token_id: u64, to_token_id: u64 }
        => Some(16),

        // Unknown — newer Jupiter version added a variant we don't know about.
        // Fail gracefully rather than mis-parsing.
        _ => None,
    }
}

// ── Borsh primitives ──────────────────────────────────────────────────────────

pub(crate) fn read_u8(data: &[u8], off: &mut usize) -> Option<u8> {
    let b = *data.get(*off)?;
    *off += 1;
    Some(b)
}

fn read_u16(data: &[u8], off: &mut usize) -> Option<u16> {
    let b: [u8; 2] = data.get(*off..*off + 2)?.try_into().ok()?;
    *off += 2;
    Some(u16::from_le_bytes(b))
}

fn read_u32(data: &[u8], off: &mut usize) -> Option<u32> {
    let b: [u8; 4] = data.get(*off..*off + 4)?.try_into().ok()?;
    *off += 4;
    Some(u32::from_le_bytes(b))
}

fn read_u64(data: &[u8], off: &mut usize) -> Option<u64> {
    let b: [u8; 8] = data.get(*off..*off + 8)?.try_into().ok()?;
    *off += 8;
    Some(u64::from_le_bytes(b))
}

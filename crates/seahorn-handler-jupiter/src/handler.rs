use seahorn_core::{ChangeSet, EntityChange, Handler, RawInstruction, SubstrateEvent, Value};

use crate::{
    decode::{
        decode_exact_out, decode_route, decode_shared_exact_out, decode_shared_route,
    },
    discriminators,
    JUPITER_V6_PROGRAM_ID_BYTES,
};

pub struct JupiterV6Handler;

impl Handler for JupiterV6Handler {
    fn handle(&self, event: &SubstrateEvent) -> ChangeSet {
        let mut cs = ChangeSet::empty(event.slot, event.step, event.cursor.clone());

        for ix in &event.instructions {
            if ix.program_id != JUPITER_V6_PROGRAM_ID_BYTES.as_slice() {
                continue;
            }
            if let Some(change) = decode_instruction(ix) {
                cs.changes.push(change);
            }
        }

        cs
    }
}

fn decode_instruction(ix: &RawInstruction) -> Option<EntityChange> {
    if ix.data.len() < 8 {
        return None;
    }
    let disc = &ix.data[..8];

    if disc == discriminators::shared_accounts_route() {
        let d = decode_shared_route(&ix.data, &ix.accounts)?;
        Some(EntityChange::Upsert {
            entity_type: "JupiterSwap",
            id: bs58::encode(&d.user).into_string(),
            fields: swap_fields(
                &d.user,
                d.source_mint.as_deref(),
                d.destination_mint.as_deref(),
                d.in_amount,
                d.quoted_out_amount,
                d.slippage_bps,
                d.platform_fee_bps,
                d.hops,
                false,
            ),
        })
    } else if disc == discriminators::route() || disc == discriminators::route_with_token_ledger() {
        let d = decode_route(&ix.data, &ix.accounts)?;
        Some(EntityChange::Upsert {
            entity_type: "JupiterSwap",
            id: bs58::encode(&d.user).into_string(),
            fields: swap_fields(
                &d.user,
                d.source_mint.as_deref(),
                d.destination_mint.as_deref(),
                d.in_amount,
                d.quoted_out_amount,
                d.slippage_bps,
                d.platform_fee_bps,
                d.hops,
                false,
            ),
        })
    } else if disc == discriminators::shared_accounts_exact_out_route() {
        let d = decode_shared_exact_out(&ix.data, &ix.accounts)?;
        Some(EntityChange::Upsert {
            entity_type: "JupiterSwap",
            id: bs58::encode(&d.user).into_string(),
            fields: swap_fields(
                &d.user,
                d.source_mint.as_deref(),
                d.destination_mint.as_deref(),
                d.quoted_in_amount,
                d.out_amount,
                d.slippage_bps,
                d.platform_fee_bps,
                d.hops,
                true,
            ),
        })
    } else if disc == discriminators::exact_out_route() {
        let d = decode_exact_out(&ix.data, &ix.accounts)?;
        Some(EntityChange::Upsert {
            entity_type: "JupiterSwap",
            id: bs58::encode(&d.user).into_string(),
            fields: swap_fields(
                &d.user,
                d.source_mint.as_deref(),
                d.destination_mint.as_deref(),
                d.quoted_in_amount,
                d.out_amount,
                d.slippage_bps,
                d.platform_fee_bps,
                d.hops,
                true,
            ),
        })
    } else {
        None
    }
}

fn swap_fields(
    user: &[u8],
    source_mint: Option<&[u8]>,
    destination_mint: Option<&[u8]>,
    in_amount: u64,
    out_amount: u64,
    slippage_bps: u16,
    platform_fee_bps: u8,
    hops: u8,
    exact_out: bool,
) -> Vec<(&'static str, Value)> {
    let mut fields = vec![
        ("user",             Value::String(bs58::encode(user).into_string())),
        ("in_amount",        Value::U64(in_amount)),
        ("out_amount",       Value::U64(out_amount)),
        ("slippage_bps",     Value::U64(slippage_bps as u64)),
        ("platform_fee_bps", Value::U64(platform_fee_bps as u64)),
        ("hops",             Value::U64(hops as u64)),
        ("exact_out",        Value::Bool(exact_out)),
    ];
    if let Some(m) = source_mint {
        fields.push(("source_mint", Value::String(bs58::encode(m).into_string())));
    }
    if let Some(m) = destination_mint {
        fields.push(("destination_mint", Value::String(bs58::encode(m).into_string())));
    }
    fields
}

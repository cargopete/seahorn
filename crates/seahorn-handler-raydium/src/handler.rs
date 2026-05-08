use seahorn_core::{ChangeSet, EntityChange, Handler, RawInstruction, SubstrateEvent, Value};

use crate::{
    decode::{decode_change_liquidity, decode_open_position, decode_swap},
    discriminators,
    RAYDIUM_CLMM_PROGRAM_ID_BYTES,
};

pub struct RaydiumClmmHandler;

impl Handler for RaydiumClmmHandler {
    fn handle(&self, event: &SubstrateEvent) -> ChangeSet {
        let mut cs = ChangeSet::empty(event.slot, event.step, event.cursor.clone());

        for ix in &event.instructions {
            if ix.program_id != RAYDIUM_CLMM_PROGRAM_ID_BYTES.as_slice() {
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

    if disc == discriminators::swap() || disc == discriminators::swap_v2() {
        let d = decode_swap(&ix.data, &ix.accounts)?;
        Some(EntityChange::Upsert {
            entity_type: "RaydiumSwap",
            id: bs58::encode(&d.pool).into_string(),
            fields: vec![
                ("pool",           Value::String(bs58::encode(&d.pool).into_string())),
                ("user",           Value::String(bs58::encode(&d.user).into_string())),
                ("amount",         Value::U64(d.amount)),
                ("other_threshold", Value::U64(d.other_threshold)),
                ("sqrt_price_limit", Value::String(d.sqrt_price_limit.to_string())),
                ("is_base_input",  Value::Bool(d.is_base_input)),
            ],
        })
    } else if disc == discriminators::open_position() {
        let d = decode_open_position(&ix.data, &ix.accounts)?;
        Some(EntityChange::Upsert {
            entity_type: "RaydiumPosition",
            id: bs58::encode(&d.pool).into_string(),
            fields: vec![
                ("pool",         Value::String(bs58::encode(&d.pool).into_string())),
                ("owner",        Value::String(bs58::encode(&d.owner).into_string())),
                ("tick_lower",   Value::I64(d.tick_lower as i64)),
                ("tick_upper",   Value::I64(d.tick_upper as i64)),
                ("liquidity",    Value::String(d.liquidity.to_string())),
                ("amount_0_max", Value::U64(d.amount_0_max)),
                ("amount_1_max", Value::U64(d.amount_1_max)),
            ],
        })
    } else if disc == discriminators::increase_liquidity() || disc == discriminators::increase_liq_v2() {
        let d = decode_change_liquidity(&ix.data, &ix.accounts)?;
        Some(EntityChange::Upsert {
            entity_type: "RaydiumAddLiquidity",
            id: bs58::encode(&d.pool).into_string(),
            fields: vec![
                ("pool",      Value::String(bs58::encode(&d.pool).into_string())),
                ("owner",     Value::String(bs58::encode(&d.owner).into_string())),
                ("liquidity", Value::String(d.liquidity.to_string())),
                ("amount_0_max", Value::U64(d.amount_0)),
                ("amount_1_max", Value::U64(d.amount_1)),
            ],
        })
    } else if disc == discriminators::decrease_liquidity() || disc == discriminators::decrease_liq_v2() {
        let d = decode_change_liquidity(&ix.data, &ix.accounts)?;
        Some(EntityChange::Upsert {
            entity_type: "RaydiumRemoveLiquidity",
            id: bs58::encode(&d.pool).into_string(),
            fields: vec![
                ("pool",      Value::String(bs58::encode(&d.pool).into_string())),
                ("owner",     Value::String(bs58::encode(&d.owner).into_string())),
                ("liquidity", Value::String(d.liquidity.to_string())),
                ("amount_0_min", Value::U64(d.amount_0)),
                ("amount_1_min", Value::U64(d.amount_1)),
            ],
        })
    } else {
        None
    }
}

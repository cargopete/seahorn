use seahorn_core::{ChangeSet, EntityChange, Handler, RawInstruction, SubstrateEvent, Value};

use crate::{
    decode::{decode_change_liquidity, decode_open_position, decode_swap},
    discriminators,
    RAYDIUM_CLMM_PROGRAM_ID_BYTES,
};

pub struct RaydiumClmmHandler;

impl Handler for RaydiumClmmHandler {
    fn handle(&self, event: &SubstrateEvent) -> ChangeSet {
        let mut cs = ChangeSet::empty(event.slot, event.signature.clone(), event.step, event.cursor.clone());

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

#[cfg(test)]
mod tests {
    use super::*;
    use seahorn_core::{Cursor, Step};
    use sha2::{Digest, Sha256};

    fn anchor_disc(name: &str) -> [u8; 8] {
        let mut h = Sha256::new();
        h.update(format!("global:{name}"));
        h.finalize()[..8].try_into().unwrap()
    }

    fn dummy_pubkey(n: u8) -> Vec<u8> { vec![n; 32] }

    fn make_event(disc: [u8; 8], extra: Vec<u8>, accounts: Vec<Vec<u8>>) -> SubstrateEvent {
        let program_id = RAYDIUM_CLMM_PROGRAM_ID_BYTES.to_vec();
        let mut data = disc.to_vec();
        data.extend(extra);
        SubstrateEvent {
            slot: 1,
            signature: vec![0u8; 64],
            step: Step::New,
            cursor: Cursor(1u64.to_le_bytes().to_vec()),
            instructions: vec![RawInstruction { program_id, data, accounts }],
        }
    }

    fn swap_args() -> Vec<u8> {
        // amount(8) other_threshold(8) sqrt_price_limit(16) is_base_input(1)
        let mut v = vec![];
        v.extend_from_slice(&1_000_000u64.to_le_bytes());
        v.extend_from_slice(&900_000u64.to_le_bytes());
        v.extend_from_slice(&0u128.to_le_bytes());
        v.push(1u8); // is_base_input = true
        v
    }

    #[test]
    fn swap_discriminator_produces_upsert() {
        let disc = anchor_disc("swap");
        let accounts: Vec<Vec<u8>> = (0..14).map(|i| dummy_pubkey(i)).collect();
        let cs = RaydiumClmmHandler.handle(&make_event(disc, swap_args(), accounts));
        assert_eq!(cs.changes.len(), 1);
        assert!(matches!(&cs.changes[0], EntityChange::Upsert { entity_type: "RaydiumSwap", .. }));
    }

    #[test]
    fn open_position_discriminator_produces_upsert() {
        let disc = anchor_disc("open_position");
        // tick_lower(4) tick_upper(4) tick_arr_lower(4) tick_arr_upper(4) liquidity(16) amt0(8) amt1(8)
        let mut args = vec![];
        args.extend_from_slice(&(-100i32).to_le_bytes());
        args.extend_from_slice(&100i32.to_le_bytes());
        args.extend_from_slice(&(-128i32).to_le_bytes());
        args.extend_from_slice(&64i32.to_le_bytes());
        args.extend_from_slice(&1_000_000u128.to_le_bytes());
        args.extend_from_slice(&500_000u64.to_le_bytes());
        args.extend_from_slice(&500_000u64.to_le_bytes());
        let accounts: Vec<Vec<u8>> = (0..18).map(|i| dummy_pubkey(i)).collect();
        let cs = RaydiumClmmHandler.handle(&make_event(disc, args, accounts));
        assert_eq!(cs.changes.len(), 1);
        assert!(matches!(&cs.changes[0], EntityChange::Upsert { entity_type: "RaydiumPosition", .. }));
    }

    #[test]
    fn unknown_discriminator_returns_empty() {
        let disc = [0xff, 0xfe, 0xfd, 0xfc, 0x00, 0x01, 0x02, 0x03];
        let accounts: Vec<Vec<u8>> = (0..14).map(|i| dummy_pubkey(i)).collect();
        let cs = RaydiumClmmHandler.handle(&make_event(disc, vec![0u8; 33], accounts));
        assert!(cs.is_empty());
    }
}

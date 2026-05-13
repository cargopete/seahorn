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
        let mut cs = ChangeSet::empty(event.slot, event.signature.clone(), event.step, event.cursor.clone());

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
        let program_id = JUPITER_V6_PROGRAM_ID_BYTES.to_vec();
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

    /// Build a minimal shared_accounts_route payload with zero hops.
    ///
    /// Layout after discriminator:
    ///   id(1) route_plan_len(4=0) in_amount(8) quoted_out(8) slippage_bps(2) fee_bps(1)
    fn shared_route_args() -> Vec<u8> {
        let mut v = vec![0u8]; // id
        v.extend_from_slice(&0u32.to_le_bytes()); // route_plan: 0 hops
        v.extend_from_slice(&1_000_000u64.to_le_bytes()); // in_amount
        v.extend_from_slice(&990_000u64.to_le_bytes());   // quoted_out_amount
        v.extend_from_slice(&50u16.to_le_bytes());        // slippage_bps
        v.push(0u8);                                       // platform_fee_bps
        v
    }

    #[test]
    fn shared_accounts_route_produces_upsert() {
        let disc = anchor_disc("shared_accounts_route");
        // need at least 9 accounts (indices 0-8)
        let accounts: Vec<Vec<u8>> = (0..9).map(|i| dummy_pubkey(i)).collect();
        let cs = JupiterV6Handler.handle(&make_event(disc, shared_route_args(), accounts));
        assert_eq!(cs.changes.len(), 1);
        assert!(matches!(&cs.changes[0], EntityChange::Upsert { entity_type: "JupiterSwap", .. }));
    }

    #[test]
    fn unknown_discriminator_returns_empty() {
        let disc = [0xaa, 0xbb, 0xcc, 0xdd, 0x00, 0x01, 0x02, 0x03];
        let accounts: Vec<Vec<u8>> = (0..9).map(|i| dummy_pubkey(i)).collect();
        let cs = JupiterV6Handler.handle(&make_event(disc, vec![0u8; 24], accounts));
        assert!(cs.is_empty());
    }
}

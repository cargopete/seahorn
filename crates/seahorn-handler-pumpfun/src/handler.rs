use seahorn_core::{ChangeSet, EntityChange, Handler, SubstrateEvent, Value};
use crate::{decode, PumpfunInstruction, PUMPFUN_PROGRAM_ID};

pub struct PumpfunHandler;

impl Handler for PumpfunHandler {
    fn handle(&self, event: &SubstrateEvent) -> ChangeSet {
        let program_id = bs58::decode(PUMPFUN_PROGRAM_ID).into_vec().unwrap_or_default();
        let sig = bs58::encode(&event.signature).into_string();
        let mut cs = ChangeSet::empty(event.slot, event.signature.clone(), event.step, event.cursor.clone());

        for ix in &event.instructions {
            if ix.program_id != program_id {
                continue;
            }
            let Some(decoded) = decode::decode(ix) else {
                continue;
            };

            let change = match decoded {
                PumpfunInstruction::Buy(b) => EntityChange::Upsert {
                    entity_type: "Buy",
                    id: format!("{}-{}", &sig[..8], event.slot),
                    fields: vec![
                        ("slot",         Value::from(event.slot)),
                        ("signature",    Value::from(sig.clone())),
                        ("mint",         Value::from(b.mint)),
                        ("user",         Value::from(b.user)),
                        ("token_amount", Value::from(b.token_amount)),
                        ("sol_cost",     Value::from(b.max_sol_cost)),
                    ],
                },
                PumpfunInstruction::Sell(s) => EntityChange::Upsert {
                    entity_type: "Sell",
                    id: format!("{}-{}", &sig[..8], event.slot),
                    fields: vec![
                        ("slot",          Value::from(event.slot)),
                        ("signature",     Value::from(sig.clone())),
                        ("mint",          Value::from(s.mint)),
                        ("user",          Value::from(s.user)),
                        ("token_amount",  Value::from(s.token_amount)),
                        ("sol_output",    Value::from(s.min_sol_output)),
                    ],
                },
                PumpfunInstruction::Create(c) => EntityChange::Upsert {
                    entity_type: "Create",
                    id: format!("{}-{}", &sig[..8], event.slot),
                    fields: vec![
                        ("slot",      Value::from(event.slot)),
                        ("signature", Value::from(sig.clone())),
                        ("mint",      Value::from(c.mint)),
                        ("creator",   Value::from(c.creator)),
                        ("name",      Value::from(c.name)),
                        ("symbol",    Value::from(c.symbol)),
                    ],
                },
            };

            cs = cs.push(change);
        }

        cs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use seahorn_core::{Cursor, RawInstruction, Step};
    use sha2::{Digest, Sha256};

    fn anchor_disc(name: &str) -> [u8; 8] {
        let mut h = Sha256::new();
        h.update(format!("global:{name}"));
        h.finalize()[..8].try_into().unwrap()
    }

    fn dummy_pubkey(n: u8) -> Vec<u8> { vec![n; 32] }

    fn make_event(disc: [u8; 8], mut args: Vec<u8>, accounts: Vec<Vec<u8>>) -> SubstrateEvent {
        let program_id = bs58::decode(PUMPFUN_PROGRAM_ID).into_vec().unwrap();
        let mut data = disc.to_vec();
        data.append(&mut args);
        SubstrateEvent {
            slot: 1,
            signature: vec![0u8; 64],
            step: Step::New,
            cursor: Cursor(1u64.to_le_bytes().to_vec()),
            instructions: vec![RawInstruction { program_id, data, accounts }],
        }
    }

    #[test]
    fn buy_discriminator_produces_upsert() {
        let disc = anchor_disc("buy");
        let mut args = vec![];
        args.extend_from_slice(&1_000_000u64.to_le_bytes()); // token_amount
        args.extend_from_slice(&500_000u64.to_le_bytes());   // max_sol_cost
        let accounts: Vec<Vec<u8>> = (0..12).map(|i| dummy_pubkey(i)).collect();

        let cs = PumpfunHandler.handle(&make_event(disc, args, accounts));

        assert_eq!(cs.changes.len(), 1);
        assert!(matches!(&cs.changes[0], EntityChange::Upsert { entity_type: "Buy", .. }));
    }

    #[test]
    fn sell_discriminator_produces_upsert() {
        let disc = anchor_disc("sell");
        let mut args = vec![];
        args.extend_from_slice(&2_000_000u64.to_le_bytes()); // token_amount
        args.extend_from_slice(&100_000u64.to_le_bytes());   // min_sol_output
        let accounts: Vec<Vec<u8>> = (0..12).map(|i| dummy_pubkey(i)).collect();

        let cs = PumpfunHandler.handle(&make_event(disc, args, accounts));

        assert_eq!(cs.changes.len(), 1);
        assert!(matches!(&cs.changes[0], EntityChange::Upsert { entity_type: "Sell", .. }));
    }

    #[test]
    fn unknown_discriminator_returns_empty() {
        let disc = [0xde, 0xad, 0xbe, 0xef, 0x00, 0x01, 0x02, 0x03];
        let args = vec![0u8; 16];
        let accounts: Vec<Vec<u8>> = (0..12).map(|i| dummy_pubkey(i)).collect();

        let cs = PumpfunHandler.handle(&make_event(disc, args, accounts));
        assert!(cs.is_empty());
    }

    #[test]
    fn wrong_program_id_returns_empty() {
        let disc = anchor_disc("buy");
        let mut args = vec![];
        args.extend_from_slice(&1u64.to_le_bytes());
        args.extend_from_slice(&1u64.to_le_bytes());
        let event = SubstrateEvent {
            slot: 1,
            signature: vec![0u8; 64],
            step: Step::New,
            cursor: Cursor(1u64.to_le_bytes().to_vec()),
            instructions: vec![RawInstruction {
                program_id: vec![0u8; 32], // wrong program
                data: { let mut d = disc.to_vec(); d.extend_from_slice(&args); d },
                accounts: (0..12).map(|i| dummy_pubkey(i)).collect(),
            }],
        };
        assert!(PumpfunHandler.handle(&event).is_empty());
    }
}

use seahorn_core::{ChangeSet, EntityChange, Handler, SubstrateEvent, Value};
use crate::{decode, PumpfunInstruction, PUMPFUN_PROGRAM_ID};

pub struct PumpfunHandler;

impl Handler for PumpfunHandler {
    fn handle(&self, event: &SubstrateEvent) -> ChangeSet {
        let program_id = bs58::decode(PUMPFUN_PROGRAM_ID).into_vec().unwrap_or_default();
        let sig = bs58::encode(&event.signature).into_string();
        let mut cs = ChangeSet::empty(event.slot, event.step, event.cursor.clone());

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

/// Position in the fork resolution lifecycle of an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// New event on the current fork.
    New,
    /// Roll back a previously emitted `New` event (fork reorg).
    Undo,
    /// Permanently on the canonical chain — safe to promote in the sink.
    Irreversible,
}

/// Opaque substrate cursor. Substrates use this to resume from a known position.
/// Consumers never inspect the bytes — only pass them back to `Substrate::stream`.
#[derive(Debug, Clone, Default)]
pub struct Cursor(pub Vec<u8>);

/// Commitment level carried by a `ChangeSet`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Commitment {
    Confirmed,
    Finalized,
}

/// A typed value for the entity store.
#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    U64(u64),
    I64(i64),
    Bool(bool),
    Bytes(Vec<u8>),
    Null,
}

impl From<&str> for Value {
    fn from(s: &str) -> Self { Value::String(s.to_owned()) }
}
impl From<String> for Value {
    fn from(s: String) -> Self { Value::String(s) }
}
impl From<u64> for Value {
    fn from(n: u64) -> Self { Value::U64(n) }
}
impl From<i64> for Value {
    fn from(n: i64) -> Self { Value::I64(n) }
}
impl From<bool> for Value {
    fn from(b: bool) -> Self { Value::Bool(b) }
}
impl From<Vec<u8>> for Value {
    fn from(b: Vec<u8>) -> Self { Value::Bytes(b) }
}

/// A single mutation to the entity store.
#[derive(Debug, Clone)]
pub enum EntityChange {
    Upsert {
        entity_type: &'static str,
        id: String,
        fields: Vec<(&'static str, Value)>,
    },
    Delete {
        entity_type: &'static str,
        id: String,
    },
}

/// The output of a pure handler function.
///
/// A `ChangeSet` is a *description* of what should change — it contains zero I/O.
/// The `Sink` is responsible for actually applying it to storage. This separation
/// is what makes handlers deterministic and testable, and what makes PoI possible
/// in a future Horizon integration.
#[derive(Debug, Clone)]
pub struct ChangeSet {
    pub slot: u64,
    pub step: Step,
    pub cursor: Cursor,
    pub changes: Vec<EntityChange>,
}

impl ChangeSet {
    pub fn empty(slot: u64, step: Step, cursor: Cursor) -> Self {
        Self { slot, step, cursor, changes: Vec::new() }
    }

    pub fn push(mut self, change: EntityChange) -> Self {
        self.changes.push(change);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

/// A raw decoded instruction as emitted by the substrate layer.
#[derive(Debug, Clone)]
pub struct RawInstruction {
    /// The program that owns this instruction (32-byte pubkey).
    pub program_id: Vec<u8>,
    /// The raw instruction data bytes.
    pub data: Vec<u8>,
    /// The resolved account keys referenced by this instruction.
    pub accounts: Vec<Vec<u8>>,
}

/// A substrate event — one confirmed (or undone) transaction with its instructions.
///
/// This is the input to every `Handler`. The substrate layer is responsible for
/// populating it correctly; handlers must not perform I/O and must return the
/// same `ChangeSet` for the same `SubstrateEvent` every time.
#[derive(Debug, Clone)]
pub struct SubstrateEvent {
    pub slot: u64,
    pub signature: Vec<u8>,
    pub step: Step,
    pub cursor: Cursor,
    pub instructions: Vec<RawInstruction>,
}

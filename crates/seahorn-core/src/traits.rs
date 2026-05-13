use futures::Stream;
use anyhow::Result;
use crate::{ChangeSet, Cursor, SubstrateEvent};

/// A source of substrate events.
///
/// Implementations: `MockSubstrate`, `YellowstoneSubstrate`, `FirehoseSubstrate`.
/// The runtime calls `stream()` once and drives the returned `Stream` to completion,
/// passing each event to the configured `Handler`.
pub trait Substrate {
    fn stream(
        &self,
        from: Option<Cursor>,
    ) -> impl Stream<Item = Result<SubstrateEvent>> + Send + '_;
}

/// A pure handler function.
///
/// Given a `SubstrateEvent`, returns the `ChangeSet` describing what should be
/// written to the entity store. Must be deterministic and I/O-free — the same
/// event must always produce the same `ChangeSet`.
pub trait Handler: Send + Sync {
    fn handle(&self, event: &SubstrateEvent) -> ChangeSet;
}

impl Handler for Box<dyn Handler> {
    fn handle(&self, event: &SubstrateEvent) -> ChangeSet {
        (**self).handle(event)
    }
}

impl<H: Handler> Handler for &H {
    fn handle(&self, event: &SubstrateEvent) -> ChangeSet {
        (*self).handle(event)
    }
}

/// Runs multiple handlers against the same event and merges their ChangeSets.
///
/// Each handler filters by its own program_id and skips instructions it doesn't
/// recognise — so wiring both `PumpfunHandler` and `RaydiumClmmHandler` into a
/// `MultiHandler` is safe and correct.
pub struct MultiHandler {
    handlers: Vec<Box<dyn Handler>>,
}

impl MultiHandler {
    pub fn new(handlers: Vec<Box<dyn Handler>>) -> Self {
        Self { handlers }
    }
}

impl Handler for MultiHandler {
    fn handle(&self, event: &SubstrateEvent) -> ChangeSet {
        let mut cs = ChangeSet::empty(event.slot, event.signature.clone(), event.step, event.cursor.clone());
        for h in &self.handlers {
            cs.changes.extend(h.handle(event).changes);
        }
        cs
    }
}

/// A sink that applies a `ChangeSet` to a storage backend.
///
/// Implementations: `StdoutSink` (dev), `PostgresSink` (v1), `KafkaSink` (v2).
/// The sink owns all I/O — handlers never touch storage directly.
pub trait Sink: Send + Sync {
    fn apply(&self, changeset: &ChangeSet) -> impl std::future::Future<Output = Result<()>> + Send;
}

impl<K: Sink> Sink for &K {
    fn apply(&self, changeset: &ChangeSet) -> impl std::future::Future<Output = Result<()>> + Send {
        (*self).apply(changeset)
    }
}

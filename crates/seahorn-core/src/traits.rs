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

/// A sink that applies a `ChangeSet` to a storage backend.
///
/// Implementations: `StdoutSink` (dev), `PostgresSink` (v1), `KafkaSink` (v2).
/// The sink owns all I/O — handlers never touch storage directly.
pub trait Sink: Send + Sync {
    fn apply(&self, changeset: &ChangeSet) -> impl std::future::Future<Output = Result<()>> + Send;
}

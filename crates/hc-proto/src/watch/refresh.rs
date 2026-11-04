// Derived from herdr-core crates/herdr-core/src/remote.rs (Apache-2.0).

//! Refresh machinery: the per-document scheduler state, the capped
//! reconnect backoff, and the three bounded fetch futures. Every fetch
//! runs under its own deadline (`WatchParams`) and carries the
//! connection epoch it was sent on, so a result from a dead connection
//! can never land as current.

use super::{DocDone, DocKey, Document};
use crate::client::{CallError, Client};
use crate::doc::DocGetParams;
use crate::machine::MachineListResult;
use crate::method;
use crate::terminal::{ListParams, ListResult};
use std::time::Duration;
use tokio::time::Instant;

/// One watched document's scheduler state: just the published
/// `Document`. The generation invalidating in-flight results lives in
/// `Actor::doc_gens` — it must survive this slot being deleted with a
/// machine.
#[derive(Debug, Default)]
pub(crate) struct DocSlot {
    pub doc: Document,
}

/// One bounded `doc.get`. `DocDone` carries the two staleness tags:
/// `rpc_epoch` (the connection this was sent on) and `fetch_gen` (which
/// request for this document it answers).
pub(crate) async fn doc_fetch(
    client: Client,
    key: DocKey,
    rpc_epoch: u64,
    fetch_gen: u64,
    timeout: Duration,
) -> DocDone {
    let params = DocGetParams {
        name: key.name.clone(),
        on: key.machine.clone(),
    };
    let result = match serde_json::to_value(&params).map_err(CallError::Shape) {
        Ok(params) => match client
            .call_timeout(method::DOC_GET, Some(params), timeout)
            .await
        {
            Ok(value) => serde_json::from_value(value).map_err(CallError::Shape),
            Err(e) => Err(e),
        },
        Err(e) => Err(e),
    };
    DocDone {
        key,
        rpc_epoch,
        fetch_gen,
        result,
    }
}

/// A bounded `machine.list`. The caller tags the spawn with the epoch.
pub(crate) async fn machine_fetch(
    client: Client,
    timeout: Duration,
) -> Result<MachineListResult, CallError> {
    match client
        .call_timeout(method::MACHINE_LIST, None, timeout)
        .await
    {
        Ok(value) => serde_json::from_value(value).map_err(CallError::Shape),
        Err(e) => Err(e),
    }
}

/// A bounded `terminal.list` — the static fallback for a core without
/// `events.subscribe`.
pub(crate) async fn static_fetch(
    client: Client,
    timeout: Duration,
) -> Result<ListResult, CallError> {
    let params = serde_json::to_value(ListParams::default()).map_err(CallError::Shape)?;
    match client
        .call_timeout(method::TERMINAL_LIST, Some(params), timeout)
        .await
    {
        Ok(value) => serde_json::from_value(value).map_err(CallError::Shape),
        Err(e) => Err(e),
    }
}

/// Reconnect backoff: doubles per failure up to a ceiling, armed lazily
/// — a `None` wake means the link is healthy and nothing is scheduled.
/// Mirrors the core's own 250 ms → 30 s reconnect in `remote.rs`.
#[derive(Debug)]
pub(crate) struct Backoff {
    initial: Duration,
    delay: Duration,
    max: Duration,
    /// When the next attempt may start; consumed as the attempt spawns.
    wake: Option<Instant>,
    /// Attempts made; surfaced in `Link`'s `attempt` count.
    pub attempts: u32,
}

impl Backoff {
    pub(crate) fn new(initial: Duration, max: Duration) -> Backoff {
        Backoff {
            initial,
            delay: initial,
            max,
            wake: None,
            attempts: 0,
        }
    }

    /// A retryable failure: next attempt after the current delay, which
    /// then doubles (capped).
    pub(crate) fn schedule(&mut self) {
        self.wake = Some(Instant::now() + self.delay);
        self.delay = (self.delay * 2).min(self.max);
    }

    /// The next attempt is due immediately — startup, or an explicit
    /// re-request.
    pub(crate) fn arm_now(&mut self) {
        self.wake = Some(Instant::now());
    }

    /// A healthy link: nothing scheduled, delay reset for the next drop.
    pub(crate) fn reset(&mut self) {
        self.wake = None;
        self.delay = self.initial;
    }

    /// `true` once, consuming the wake: an attempt is now in flight and
    /// must not be spawned twice while it runs.
    pub(crate) fn take_due(&mut self) -> bool {
        if self.wake.is_some_and(|at| at <= Instant::now()) {
            self.wake = None;
            true
        } else {
            false
        }
    }

    /// When the next attempt is due, if one is scheduled.
    pub(crate) fn wake_at(&self) -> Option<Instant> {
        self.wake
    }
}

// Derived from herdr-core crates/herdr-core/src/remote.rs (Apache-2.0).

//! The actor's state handlers: wire notifications through the reducer
//! plus facility death and install. Fetch completions and the refresh
//! queues they feed live in `fetch_done.rs`. All `impl Actor` — split
//! from `mod.rs` at the event-loop/state-application seam.

use super::Actor;
use crate::client::Client;
use crate::events::ServerNotification;
use crate::inventory::Effect;
use crate::subscribe::SubscribeError;
use crate::watch::{DocKey, Link, SubAttempt};
use std::sync::Arc;
use tokio::task::JoinError;

impl Actor {
    /// A subscription wire message: apply it through the reviewed
    /// reducer and translate its effects into refreshes. `resync` means
    /// the stream can no longer be trusted — a fresh subscription
    /// replaces it.
    pub(super) fn notification(&mut self, note: Result<ServerNotification, SubscribeError>) {
        match note {
            Ok(ServerNotification::Event(event)) => {
                // A stale or duplicate `seq` is a no-op for `apply` —
                // skip the copy-on-write clone entirely.
                let effect = if event.seq <= self.inventory.seq() {
                    Effect::default()
                } else {
                    Arc::make_mut(&mut self.inventory).apply(&event)
                };
                if effect.resync || effect.malformed {
                    self.kill_sub("event stream requires resync");
                }
                for doc in &effect.docs {
                    if self.params.documents.contains(&doc.name) {
                        self.request_doc(DocKey {
                            machine: doc.machine.clone(),
                            name: doc.name.clone(),
                        });
                    }
                }
                // A machine has no event of its own; membership changes
                // are the closest signal a machine list changed.
                if !effect.created.is_empty() || !effect.removed.is_empty() {
                    self.machines_pending = true;
                }
                self.publish();
            }
            Ok(ServerNotification::Heartbeat(heartbeat)) => {
                // `heartbeat` only mutates when it detects a gap: the
                // healthy cadence keeps the shared inventory untouched.
                let effect = if heartbeat.seq > self.inventory.seq() && !self.inventory.filtered() {
                    Arc::make_mut(&mut self.inventory).heartbeat(&heartbeat)
                } else {
                    Effect::default()
                };
                if effect.resync {
                    self.kill_sub("heartbeat beyond the observed stream");
                    self.publish();
                } else {
                    // Core heartbeats about every 15 s: that cadence —
                    // not a client-side timer — bounds machine-list
                    // freshness, coalesced by the in-flight flag.
                    self.machines_pending = true;
                }
            }
            Ok(ServerNotification::Missed(missed)) => {
                Arc::make_mut(&mut self.inventory).missed(&missed);
                self.refresh_docs();
                self.machines_pending = true;
                self.publish();
            }
            Err(e) => {
                self.kill_sub(&e.to_string());
                self.publish();
            }
        }
    }

    /// Ends the subscription epoch — the stream died or its data can no
    /// longer be trusted. The last inventory stays, marked offline, and
    /// a reconnect is scheduled.
    fn kill_sub(&mut self, reason: &str) {
        if self.sub_conn.take().is_none() && matches!(self.link, Link::Offline { .. }) {
            return; // already torn down
        }
        if !self.inventory.is_offline() {
            Arc::make_mut(&mut self.inventory).set_offline(true);
        }
        self.link = Link::Offline {
            attempt: self.sub_backoff.attempts,
            error: reason.into(),
        };
        self.sub_backoff.schedule();
    }

    /// The client's `closed` latch fired: mark the link, invalidate the
    /// epoch and abort every fetch still holding the dead connection.
    pub(super) fn rpc_died(&mut self) {
        if self.client.take().is_none() {
            return;
        }
        self.rpc_epoch += 1;
        self.fetches.abort_all();
        self.doc_inflight.clear();
        if self.docs.values().any(|doc| doc.fetching) {
            for doc in Arc::make_mut(&mut self.docs).values_mut() {
                if doc.fetching {
                    Arc::make_mut(doc).fetching = false;
                }
            }
        }
        if let Some(handle) = self.machine_fetch.take() {
            handle.abort();
            self.machines_pending = true;
        }
        if let Some(handle) = self.static_fetch.take() {
            handle.abort();
            self.static_pending = true;
        }
        self.rpc_link = Link::Offline {
            attempt: self.rpc_backoff.attempts,
            error: "connection lost".into(),
        };
        self.notes.insert("rpc".into(), "connection lost".into());
        self.rpc_backoff.schedule();
        self.publish();
    }

    /// A subscription open attempt landed: a live epoch, the static
    /// fallback, or a failure that schedules the next backoff.
    pub(super) fn install_sub(&mut self, result: Result<SubAttempt, JoinError>) {
        self.sub_open = None;
        match result {
            Ok(SubAttempt::Subscribed(sub)) => {
                self.generation += 1;
                self.inventory = Arc::new(sub.inventory());
                self.core = Some(sub.hello().core.clone());
                self.features = sub.hello().features.clone();
                self.sub_conn = Some(*sub);
                self.link = Link::Online;
                self.sub_backoff.reset();
                self.notes.remove("events.subscribe");
                self.refresh_all();
            }
            Ok(SubAttempt::Unsupported(reason)) => {
                // `events.subscribe` is gone for good on this core: keep
                // the static snapshot path, never retry the sub itself.
                self.link = Link::Static {
                    reason: reason.clone(),
                };
                self.notes.insert("events.subscribe".into(), reason);
                self.sub_backoff.reset();
                self.static_pending = self.static_enabled;
                self.refresh_all();
            }
            Ok(SubAttempt::Failed(e)) => {
                if matches!(self.link, Link::Static { .. }) {
                    // A capability re-probe failing keeps the static
                    // snapshot — not a subscription death to retry.
                } else {
                    self.link = Link::Offline {
                        attempt: self.sub_backoff.attempts,
                        error: e.to_string(),
                    };
                    self.sub_backoff.schedule();
                }
            }
            Err(join) => {
                if join.is_cancelled() {
                    return; // shutting down
                }
                self.link = Link::Offline {
                    attempt: self.sub_backoff.attempts,
                    error: "connect task died".into(),
                };
                self.sub_backoff.schedule();
            }
        }
        self.publish();
    }

    /// An RPC connect attempt landed.
    pub(super) fn install_rpc(
        &mut self,
        result: Result<Result<Client, crate::client::Error>, JoinError>,
    ) {
        self.rpc_open = None;
        match result {
            Ok(Ok(client)) => {
                self.core = Some(client.core().clone());
                self.features = client.hello().features.clone();
                self.client = Some(client);
                // A new epoch: results still flying on the dead conn are
                // stale by tag, and everything it owned gets a fresh read.
                self.rpc_epoch += 1;
                self.rpc_link = Link::Online;
                self.rpc_backoff.reset();
                self.notes.remove("rpc");
                // Fresh hello — possibly fresh capabilities: every
                // `Unsupported` feature flag re-opens and re-probes once.
                self.docs_enabled = true;
                self.machines_enabled = true;
                self.static_enabled = true;
                if matches!(self.link, Link::Static { .. }) {
                    // A core that lacked `events.subscribe` may have it
                    // now — one re-probe per rpc epoch, never a retry loop.
                    self.sub_backoff.arm_now();
                }
                self.refresh_all();
            }
            Ok(Err(e)) => {
                self.rpc_link = Link::Offline {
                    attempt: self.rpc_backoff.attempts,
                    error: e.to_string(),
                };
                self.rpc_backoff.schedule();
            }
            Err(join) => {
                if join.is_cancelled() {
                    return; // shutting down
                }
                self.rpc_link = Link::Offline {
                    attempt: self.rpc_backoff.attempts,
                    error: "connect task died".into(),
                };
                self.rpc_backoff.schedule();
            }
        }
        self.publish();
    }
}

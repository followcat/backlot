// Derived from herdr-core crates/herdr-core/src/remote.rs (Apache-2.0).

//! The actor's state handlers: wire notifications through the reducer,
//! facility death and install, fetch completions and the refresh queues
//! they feed. All `impl Actor` — split from `mod.rs` at the
//! event-loop/state-application seam.

use super::Actor;
use crate::client::{CallError, Client};
use crate::events::ServerNotification;
use crate::machine::MachineListResult;
use crate::subscribe::SubscribeError;
use crate::terminal::ListResult;
use crate::watch::{DocDone, DocKey, Link, SubAttempt};
use std::collections::BTreeSet;
use tokio::task::JoinError;

impl Actor {
    /// A subscription wire message: apply it through the reviewed
    /// reducer and translate its effects into refreshes. `resync` means
    /// the stream can no longer be trusted — a fresh subscription
    /// replaces it.
    pub(super) fn notification(&mut self, note: Result<ServerNotification, SubscribeError>) {
        match note {
            Ok(ServerNotification::Event(event)) => {
                let effect = self.inventory.apply(&event);
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
                let effect = self.inventory.heartbeat(&heartbeat);
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
                self.inventory.missed(&missed);
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
        self.inventory.set_offline(true);
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
        for slot in self.docs.values_mut() {
            slot.doc.fetching = false;
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
                self.inventory = sub.inventory();
                self.core = Some(sub.hello().core.clone());
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

    /// A bounded `doc.get` completed. Both staleness tags must still be
    /// current — the connection epoch and this document's fetch
    /// generation — or the result is discarded, never installed.
    pub(super) fn doc_done(&mut self, result: Result<DocDone, JoinError>) {
        let Ok(done) = result else { return };
        // The epoch check comes first: a completion sent on a dead
        // connection must not touch this epoch's counters either.
        if done.rpc_epoch != self.rpc_epoch {
            return;
        }
        // A completed fetch frees its machine's slot whether or not the
        // result may still land.
        if let Some(n) = self.doc_inflight.get_mut(&done.key.machine) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                self.doc_inflight.remove(&done.key.machine);
            }
        }
        if self.doc_gens.get(&done.key) != Some(&done.fetch_gen) {
            return; // superseded — a newer request is pending or in flight
        }
        let Some(slot) = self.docs.get_mut(&done.key) else {
            return; // the machine (and its slots) went away
        };
        slot.doc.fetching = false;
        match done.result {
            Ok(doc) => {
                slot.doc.value = doc.value;
                slot.doc.version = doc.version;
                slot.doc.error = None;
                self.notes.remove("doc.get");
            }
            Err(CallError::Unsupported { reason, .. }) => {
                // The method is gone for good: only this feature stops.
                self.docs_enabled = false;
                self.doc_pending.clear();
                self.notes.insert("doc.get".into(), reason.clone());
                slot.doc.error = Some(reason);
            }
            Err(e) => {
                slot.doc.error = Some(e.to_string());
            }
        }
        self.publish();
    }

    /// A bounded `machine.list` completed: installs the list, queues
    /// document fetches on newly seen machines and drops slots whose
    /// machine went away.
    pub(super) fn machines_done(
        &mut self,
        result: Result<(u64, Result<MachineListResult, CallError>), JoinError>,
    ) {
        self.machine_fetch = None;
        let Ok((epoch, result)) = result else { return };
        if epoch != self.rpc_epoch {
            return;
        }
        match result {
            Ok(list) => {
                let old: BTreeSet<String> = self
                    .machines
                    .iter()
                    .flatten()
                    .map(|m| m.name.clone())
                    .collect();
                let names: BTreeSet<String> =
                    list.machines.iter().map(|m| m.name.clone()).collect();
                for name in names.difference(&old) {
                    for doc in self.params.documents.clone() {
                        self.request_doc(DocKey {
                            machine: Some(name.clone()),
                            name: doc,
                        });
                    }
                }
                let gone: BTreeSet<String> = old.difference(&names).cloned().collect();
                if !gone.is_empty() {
                    let keep =
                        |key: &DocKey| key.machine.as_ref().is_none_or(|m| !gone.contains(m));
                    self.docs.retain(|key, _| keep(key));
                    self.doc_pending.retain(keep);
                }
                self.machines = Some(list.machines);
                self.notes.remove("machine.list");
            }
            Err(CallError::Unsupported { reason, .. }) => {
                self.machines_enabled = false;
                self.machines = None;
                self.notes.insert("machine.list".into(), reason);
            }
            Err(e) => {
                self.notes.insert("machine.list".into(), e.to_string());
            }
        }
        self.publish();
    }

    /// A bounded `terminal.list` for the static fallback completed.
    pub(super) fn static_done(
        &mut self,
        result: Result<(u64, Result<ListResult, CallError>), JoinError>,
    ) {
        self.static_fetch = None;
        let Ok((epoch, result)) = result else { return };
        if epoch != self.rpc_epoch {
            return;
        }
        match result {
            Ok(list) => {
                self.generation += 1;
                self.inventory.replace(0, list.terminals);
                // `terminal.list` carries machine views too: use them
                // when `machine.list` never produced any.
                if self.machines.is_none() && !list.machines.is_empty() {
                    for machine in &list.machines {
                        for doc in self.params.documents.clone() {
                            self.request_doc(DocKey {
                                machine: Some(machine.name.clone()),
                                name: doc,
                            });
                        }
                    }
                    self.machines = Some(list.machines);
                }
                self.notes.remove("terminal.list");
            }
            Err(CallError::Unsupported { reason, .. }) => {
                self.static_enabled = false;
                self.notes.insert("terminal.list".into(), reason);
            }
            Err(e) => {
                self.notes.insert("terminal.list".into(), e.to_string());
            }
        }
        self.publish();
    }

    /// Every watched document on this core and every known machine.
    pub(super) fn refresh_docs(&mut self) {
        let mut machines = vec![None];
        if let Some(list) = &self.machines {
            machines.extend(list.iter().map(|m| Some(m.name.clone())));
        }
        for name in self.params.documents.clone() {
            for machine in &machines {
                self.request_doc(DocKey {
                    machine: machine.clone(),
                    name: name.clone(),
                });
            }
        }
    }

    /// Queues a document fetch, bumping the document's generation now —
    /// an in-flight result becomes stale the moment a newer request
    /// exists, not only once a slot frees and the next fetch spawns.
    /// `doc_gens` outlives slot deletion, so a machine alias removed and
    /// re-added can never match an old fetch's generation again.
    pub(super) fn request_doc(&mut self, key: DocKey) {
        if !self.docs_enabled {
            return;
        }
        self.docs.entry(key.clone()).or_default();
        *self.doc_gens.entry(key.clone()).or_default() += 1;
        self.doc_pending.insert(key);
    }

    /// A `Refresh` command or a fresh facility: everything refetches.
    pub(super) fn refresh_all(&mut self) {
        self.refresh_docs();
        self.machines_pending = true;
        if matches!(self.link, Link::Static { .. }) {
            self.static_pending = true;
        }
    }
}

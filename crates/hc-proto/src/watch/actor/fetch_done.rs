// Derived from herdr-core crates/herdr-core/src/remote.rs (Apache-2.0).

//! Fetch completions (`doc.get`, `machine.list`, `terminal.list`) and
//! the refresh queues feeding them — split from `apply.rs` at the
//! wire-notification/fetch-lifecycle seam.

use super::Actor;
use crate::client::CallError;
use crate::machine::MachineListResult;
use crate::terminal::ListResult;
use crate::watch::{DocDone, DocKey, Document, Link};
use std::collections::BTreeSet;
use std::sync::Arc;
use tokio::task::JoinError;

impl Actor {
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
        if !self.docs.contains_key(&done.key) {
            return; // the machine (and its slots) went away
        }
        // Two copy-on-write steps: the map clone only when the key is
        // really present, then this document's own `Document` clone —
        // untouched entries keep their `Arc` for the next publish.
        let slot = Arc::make_mut(
            Arc::make_mut(&mut self.docs)
                .get_mut(&done.key)
                .unwrap_or_else(|| unreachable!()),
        );
        slot.fetching = false;
        match done.result {
            Ok(doc) => {
                slot.value = doc.value;
                slot.version = doc.version;
                slot.error = None;
                self.notes.remove("doc.get");
            }
            Err(CallError::Unsupported { reason, .. }) => {
                // The method is gone for good: only this feature stops.
                self.docs_enabled = false;
                self.doc_pending.clear();
                self.notes.insert("doc.get".into(), reason.clone());
                slot.error = Some(reason);
            }
            Err(e) => {
                slot.error = Some(e.to_string());
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
                    Arc::make_mut(&mut self.docs).retain(|key, _| keep(key));
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
                let inventory = Arc::make_mut(&mut self.inventory);
                inventory.replace(0, list.terminals);
                inventory.set_offline(false);
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
        // The map clone happens only when the key is really new — a
        // repeat request shares the published map untouched.
        if !self.docs.contains_key(&key) {
            Arc::make_mut(&mut self.docs).insert(key.clone(), Arc::new(Document::default()));
        }
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

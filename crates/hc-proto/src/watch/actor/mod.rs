// Derived from herdr-core crates/herdr-core/src/remote.rs (Apache-2.0).

//! The watcher's actor: one task owning both connections, the inventory
//! and every refresh. `run` is a `select!` awaiting input — a
//! notification, a wire death, a joined handle, a queued command, the
//! stop latch or a backoff deadline; nothing polls while idle.

use super::refresh::{self, Backoff, DocSlot};
use super::{
    ActivityReceipts, Command, DocDone, DocKey, Link, OpenRpc, OpenSub, SubAttempt, View,
    WatchParams,
};
use crate::client::{CallError, Client};
use crate::hello::CoreInfo;
use crate::inventory::Inventory;
use crate::machine::{MachineListResult, MachineView};
use crate::subscribe::Subscription;
use crate::terminal::ListResult;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::Instant;

mod apply;

/// In-flight `doc.get` bound per machine: a dead or slow machine can
/// never fill the whole `doc_concurrency` budget.
const DOC_PER_MACHINE: usize = 1;

/// Everything the actor owns: both facilities, the authoritative state
/// and all scheduling. Kept private to the task — the `Watcher` handle
/// only ever sees the published `View`.
pub(crate) struct Actor {
    pub(super) params: WatchParams,
    pub(super) view: Arc<Mutex<View>>,
    pub(super) version_tx: watch::Sender<u64>,
    pub(super) commands: mpsc::Receiver<Command>,
    pub(super) stop: watch::Receiver<bool>,
    pub(super) rpc: OpenRpc,
    pub(super) sub: OpenSub,
    // Live facilities: the framed RPC client, the dedicated
    // subscription, and any in-flight connect attempts for each.
    pub(super) client: Option<Client>,
    pub(super) sub_conn: Option<Subscription>,
    pub(super) rpc_open: Option<JoinHandle<Result<Client, crate::client::Error>>>,
    pub(super) sub_open: Option<JoinHandle<SubAttempt>>,
    // Authoritative state.
    pub(super) inventory: Inventory,
    activity_receipts: ActivityReceipts,
    /// Subscription epoch: bumps when a fresh snapshot (subscription or
    /// static list) replaces the inventory.
    pub(super) generation: u64,
    /// RPC epoch: bumps on every client (re)install; tags every fetch.
    pub(super) rpc_epoch: u64,
    pub(super) link: Link,
    pub(super) rpc_link: Link,
    pub(super) core: Option<CoreInfo>,
    pub(super) features: Vec<String>,
    pub(super) machines: Option<Vec<MachineView>>,
    pub(super) docs: BTreeMap<DocKey, DocSlot>,
    pub(super) notes: BTreeMap<String, String>,
    // Refresh scheduling.
    pub(super) doc_pending: BTreeSet<DocKey>,
    /// In-flight `doc.get` count per machine (`None` — this core).
    pub(super) doc_inflight: BTreeMap<Option<String>, usize>,
    /// Per-document fetch generations — kept when a slot is deleted so
    /// re-added machines can never re-match an old result's generation.
    pub(super) doc_gens: BTreeMap<DocKey, u64>,
    pub(super) fetches: JoinSet<DocDone>,
    pub(super) machines_pending: bool,
    pub(super) machines_enabled: bool,
    pub(super) machine_fetch: Option<JoinHandle<(u64, Result<MachineListResult, CallError>)>>,
    pub(super) static_pending: bool,
    pub(super) static_enabled: bool,
    pub(super) static_fetch: Option<JoinHandle<(u64, Result<ListResult, CallError>)>>,
    pub(super) docs_enabled: bool,
    // Per-facility reconnect backoff.
    pub(super) sub_backoff: Backoff,
    pub(super) rpc_backoff: Backoff,
    // Publish counter.
    pub(super) version: u64,
}

impl Actor {
    pub(crate) fn new(
        params: WatchParams,
        view: Arc<Mutex<View>>,
        version_tx: watch::Sender<u64>,
        commands: mpsc::Receiver<Command>,
        stop: watch::Receiver<bool>,
        rpc: OpenRpc,
        sub: OpenSub,
    ) -> Actor {
        Actor {
            sub_backoff: Backoff::new(params.reconnect_initial, params.reconnect_max),
            rpc_backoff: Backoff::new(params.reconnect_initial, params.reconnect_max),
            params,
            view,
            version_tx,
            commands,
            stop,
            rpc,
            sub,
            client: None,
            sub_conn: None,
            rpc_open: None,
            sub_open: None,
            inventory: Inventory::default(),
            activity_receipts: ActivityReceipts::default(),
            generation: 0,
            rpc_epoch: 0,
            link: Link::Connecting { attempt: 0 },
            rpc_link: Link::Connecting { attempt: 0 },
            core: None,
            features: vec![],
            machines: None,
            docs: BTreeMap::new(),
            notes: BTreeMap::new(),
            doc_pending: BTreeSet::new(),
            doc_inflight: BTreeMap::new(),
            doc_gens: BTreeMap::new(),
            fetches: JoinSet::new(),
            machines_pending: false,
            machines_enabled: true,
            machine_fetch: None,
            static_pending: false,
            static_enabled: true,
            static_fetch: None,
            docs_enabled: true,
            version: 0,
        }
    }

    /// The event loop: both connections' first attempts arm at once,
    /// then every turn awaits work — never polling.
    pub(crate) async fn run(mut self) {
        self.sub_backoff.arm_now();
        self.rpc_backoff.arm_now();
        loop {
            self.pump();
            let wake = self.next_wake();
            tokio::select! {
                biased;
                _ = self.stop.changed() => break,
                command = self.commands.recv() => match command {
                    Some(Command::Refresh) => self.refresh_all(),
                    None => break, // the handle is gone
                },
                note = async {
                    match self.sub_conn.as_mut() {
                        Some(sub) => sub.recv().await,
                        None => std::future::pending().await,
                    }
                } => self.notification(note),
                _ = async {
                    match self.client.as_ref() {
                        Some(client) => client.closed().await,
                        None => std::future::pending::<()>().await,
                    }
                } => self.rpc_died(),
                result = async {
                    match self.sub_open.as_mut() {
                        Some(handle) => handle.await,
                        None => std::future::pending().await,
                    }
                } => self.install_sub(result),
                result = async {
                    match self.rpc_open.as_mut() {
                        Some(handle) => handle.await,
                        None => std::future::pending().await,
                    }
                } => self.install_rpc(result),
                result = async {
                    if self.fetches.is_empty() {
                        std::future::pending().await
                    } else {
                        self.fetches.join_next().await.unwrap_or_else(|| unreachable!())
                    }
                } => self.doc_done(result),
                result = async {
                    match self.machine_fetch.as_mut() {
                        Some(handle) => handle.await,
                        None => std::future::pending().await,
                    }
                } => self.machines_done(result),
                result = async {
                    match self.static_fetch.as_mut() {
                        Some(handle) => handle.await,
                        None => std::future::pending().await,
                    }
                } => self.static_done(result),
                _ = async {
                    match wake {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending::<()>().await,
                    }
                } => {}
            }
        }
        self.shutdown();
    }

    /// Spawns due reconnect attempts, then drains pending refreshes into
    /// their bounded in-flight slots.
    fn pump(&mut self) {
        if self.sub_backoff.take_due()
            && self.sub_open.is_none()
            && self.sub_conn.is_none()
            && !matches!(self.link, Link::Stopped)
        {
            self.sub_backoff.attempts += 1;
            if matches!(self.link, Link::Connecting { .. }) {
                self.link = Link::Connecting {
                    attempt: self.sub_backoff.attempts,
                };
            }
            let open = Arc::clone(&self.sub);
            let subscribe = self.params.subscribe.clone();
            self.sub_open = Some(tokio::spawn(async move { open(subscribe).await }));
            self.publish();
        }
        if self.rpc_backoff.take_due() && self.rpc_open.is_none() && self.client.is_none() {
            self.rpc_backoff.attempts += 1;
            if matches!(self.rpc_link, Link::Connecting { .. }) {
                self.rpc_link = Link::Connecting {
                    attempt: self.rpc_backoff.attempts,
                };
            }
            let open = Arc::clone(&self.rpc);
            self.rpc_open = Some(tokio::spawn(async move { open().await }));
            self.publish();
        }
        self.drain_refreshes();
    }

    /// Moves pending refreshes into bounded in-flight work — only while
    /// the RPC link is up. Document fetches additionally need an
    /// established epoch (`generation > 0`): subscribe lands before the
    /// initial document reads.
    fn drain_refreshes(&mut self) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let list_timeout = self.params.list_timeout;
        if self.machines_pending && self.machine_fetch.is_none() && self.machines_enabled {
            self.machines_pending = false;
            let epoch = self.rpc_epoch;
            let task = client.clone();
            self.machine_fetch = Some(tokio::spawn(async move {
                (epoch, refresh::machine_fetch(task, list_timeout).await)
            }));
        }
        if self.static_pending && self.static_fetch.is_none() && self.static_enabled {
            self.static_pending = false;
            let epoch = self.rpc_epoch;
            let task = client.clone();
            self.static_fetch = Some(tokio::spawn(async move {
                (epoch, refresh::static_fetch(task, list_timeout).await)
            }));
        }
        if self.docs_enabled && matches!(self.link, Link::Online | Link::Static { .. }) {
            let doc_timeout = self.params.doc_timeout;
            while self.fetches.len() < self.params.doc_concurrency {
                // Fairness: one slow machine may hold at most
                // DOC_PER_MACHINE in-flight slots — never all of them.
                let Some(key) = self
                    .doc_pending
                    .iter()
                    .find(|key| {
                        self.doc_inflight.get(&key.machine).copied().unwrap_or(0) < DOC_PER_MACHINE
                    })
                    .cloned()
                else {
                    break;
                };
                self.doc_pending.remove(&key);
                *self.doc_inflight.entry(key.machine.clone()).or_default() += 1;
                let slot = self.docs.entry(key.clone()).or_default();
                slot.doc.fetching = true;
                let fetch_gen = self.doc_gens.get(&key).copied().unwrap_or_default();
                let fetch =
                    refresh::doc_fetch(client.clone(), key, self.rpc_epoch, fetch_gen, doc_timeout);
                self.fetches.spawn(fetch);
            }
        }
    }

    /// The sooner of the two facilities' backoff deadlines.
    fn next_wake(&self) -> Option<Instant> {
        match (self.sub_backoff.wake_at(), self.rpc_backoff.wake_at()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Rebuilds the published `View` and bumps its version. A slow
    /// reader only ever sees complete states — publish is the coalesce.
    fn publish(&mut self) {
        self.version += 1;
        let mut view = View {
            version: self.version,
            generation: self.generation,
            link: self.link.clone(),
            rpc: self.rpc_link.clone(),
            core: self.core.clone(),
            features: self.features.clone(),
            inventory: self.inventory.clone(),
            activity_receipts: self.activity_receipts.clone(),
            machines: self.machines.clone(),
            documents: self
                .docs
                .iter()
                .map(|(key, slot)| (key.clone(), slot.doc.clone()))
                .collect(),
            notes: self.notes.clone(),
        };
        // Receipts ingest on the authoritative candidate: the resolved
        // traversal orders boundary reallocation; machine lifecycle
        // retires origin slots before the new ingest extends them.
        let traversal = self
            .params
            .traversal
            .map(|resolve| resolve(&view))
            .unwrap_or_default();
        self.activity_receipts
            .sync_machines(self.machines.as_deref());
        self.activity_receipts.ingest(&self.inventory, &traversal);
        view.activity_receipts = self.activity_receipts.clone();
        *self.view.lock().unwrap_or_else(|p| p.into_inner()) = view;
        // `send_replace` stores unconditionally — `send` would drop the
        // value when no receiver exists, and a `changed_since` created
        // afterwards could never observe the publish it missed.
        self.version_tx.send_replace(self.version);
    }

    /// Drops both connections and aborts every in-flight open and
    /// fetch — synchronous drops only, so an aborted task unwinds
    /// through the same field `Drop`s (guard kill, reader/writer
    /// release) instead of a graceful path that could block.
    fn shutdown(&mut self) {
        self.abort_pending();
        self.link = Link::Stopped;
        self.rpc_link = Link::Stopped;
        self.publish();
    }

    /// Kills every spawned task and releases both facilities. A
    /// `JoinHandle` dropped without `abort` detaches — the spawned
    /// opener or fetch would keep its connection (and any ssh child)
    /// alive past the actor's death.
    fn abort_pending(&mut self) {
        if let Some(handle) = self.sub_open.take() {
            handle.abort();
        }
        if let Some(handle) = self.rpc_open.take() {
            handle.abort();
        }
        if let Some(handle) = self.machine_fetch.take() {
            handle.abort();
        }
        if let Some(handle) = self.static_fetch.take() {
            handle.abort();
        }
        self.fetches.abort_all();
        self.sub_conn.take();
        self.client.take();
    }
}

impl Drop for Actor {
    /// An aborted `run` never reaches `shutdown` — dropping still
    /// aborts every spawned handle and releases both facilities, so no
    /// detached task, connection or owned child outlives the actor.
    fn drop(&mut self) {
        self.abort_pending();
    }
}

#[cfg(test)]
mod epoch_tests;
#[cfg(test)]
mod receipt_tests;

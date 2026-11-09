// Derived from herdr-core crates/herdr-core/src/remote.rs (supervise,
// mirror_events, doc/machine refresh) and schema/README.md (Apache-2.0).

//! The reconnecting inventory watcher: one owned actor composing the
//! reviewed [`Subscription`] and pure [`Inventory`] reducer with the
//! framed [`Client`] for read-only refreshes (`doc.get`, `machine.list`,
//! `terminal.list` for the static fallback). It is the event-driven
//! state the UI consumes — never a polling timer.
//!
//! Semantics, matching the pinned core contract:
//! - The subscription is a dedicated plain connection; RPC calls share a
//!   separate framed one. Both die independently and reconnect with
//!   capped backoff — each facility on its own timer. The last inventory
//!   stays visible, marked offline, until a fresh epoch replaces it.
//! - `events.subscribe` completes *before* any document read; a pre-reply
//!   notification or malformed stream ends the facility, not the state.
//! - Snapshots (initial, `missed`, a new epoch) replace the inventory and
//!   refetch every watched document on this core and every machine
//!   `machine.list` reports. `doc` events refetch only watched names.
//! - Refresh results are tagged by connection epoch and per-document
//!   fetch generation — a stale result from before a reconnect or a
//!   newer event can never overwrite current state.
//! - Missing methods disable only their feature, with a reason kept:
//!   an unsupported subscription falls back to a `terminal.list`
//!   snapshot and does not retry in a loop.
//! - Only read-only calls are made — `doc.get`, `machine.list`,
//!   `terminal.list`. Nothing state-changing is ever replayed.

mod actor;
mod receipts;
mod refresh;

pub use receipts::ActivityReceipts;

use crate::client::{CallError, Client};
use crate::doc::DocGetResult;
use crate::endpoint::Endpoint;
use crate::events::SubscribeParams;
use crate::hello::CoreInfo;
use crate::inventory::Inventory;
use crate::machine::MachineView;
use crate::subscribe::{SubscribeError, Subscription};
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

/// A boxed future for the connection openers.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// Opens the framed RPC connection. Called at start and after every RPC
/// death — each call must create a fresh transport.
pub type OpenRpc = Arc<dyn Fn() -> BoxFuture<Result<Client, crate::client::Error>> + Send + Sync>;

/// Opens the dedicated subscription connection, fresh on every attempt.
pub type OpenSub = Arc<dyn Fn(SubscribeParams) -> BoxFuture<SubAttempt> + Send + Sync>;

/// What one subscription connect attempt produced.
pub enum SubAttempt {
    /// A live subscription: snapshot taken, notifications flow.
    Subscribed(Box<Subscription>),
    /// The core does not offer `events.subscribe` — the watcher falls
    /// back to a `terminal.list` snapshot instead of retrying forever.
    Unsupported(String),
    /// The connection or negotiation failed: a retryable death.
    Failed(SubscribeError),
}

/// What the watcher keeps in view: terminals, machines, documents and
/// the state of both connections. Rebuilt and published on every change;
/// a slow reader always sees the newest complete state — never stale
/// data presented as current.
#[derive(Debug, Clone, Default)]
pub struct View {
    /// Bumped on every publish.
    pub version: u64,
    /// The subscription epoch: a fresh snapshot's serial — bumps when a
    /// new subscription or static list replaces the inventory, so panes
    /// know to reattach rather than patch.
    pub generation: u64,
    /// The subscription link's state.
    pub link: Link,
    /// The RPC link's state.
    pub rpc: Link,
    /// The connected core's identity (`hello`), when known.
    pub core: Option<CoreInfo>,
    /// Features advertised by the most recently installed handshake.
    pub features: Vec<String>,
    /// The authoritative terminal inventory.
    pub inventory: Inventory,
    /// Display-only receipt ranks captured before publication coalesces views.
    pub activity_receipts: ActivityReceipts,
    /// `machine.list` as last seen. `None` while never fetched or the
    /// method is missing — `notes["machine.list"]` says which.
    pub machines: Option<Vec<MachineView>>,
    /// Watched documents by `(machine, name)`: the last good value plus
    /// the last fetch's error, if any.
    pub documents: BTreeMap<DocKey, Document>,
    /// Per-feature reasons for degraded state (`doc.get`, `machine.list`,
    /// `events.subscribe`, the rpc link). Removed when the feature works.
    pub notes: BTreeMap<String, String>,
}

/// One facility's connection state.
#[derive(Debug, Clone, PartialEq)]
pub enum Link {
    /// Never opened, or reconnecting after a failure (attempt count).
    Connecting {
        /// How many attempts were made so far.
        attempt: u32,
    },
    /// Live: hello and the call/subscription completed.
    Online,
    /// `events.subscribe` is unsupported: the view is a `terminal.list`
    /// snapshot — correct, but not event-driven.
    Static {
        /// Why the subscription could not be used.
        reason: String,
    },
    /// Down; a retry is scheduled. `error` is the last failure.
    Offline {
        /// How many attempts were made so far.
        attempt: u32,
        /// The last failure's description.
        error: String,
    },
    /// Explicitly stopped; nothing will reconnect.
    Stopped,
}

impl Default for Link {
    fn default() -> Link {
        Link::Connecting { attempt: 0 }
    }
}

/// A watched document's address: a name plus the machine that stores it
/// (`None` — this core; `Some("host-b")` — `doc.get`'s `on`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocKey {
    /// The machine as this core names it (`on` for `doc.get`).
    pub machine: Option<String>,
    /// The document name, e.g. `hc-tui`.
    pub name: String,
}

/// A watched document's last good read, or only an error when none
/// ever succeeded.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document {
    /// The stored JSON — opaque, never interpreted.
    pub value: Value,
    /// The document version the core reported.
    pub version: u64,
    /// A fetch is running for this document right now.
    pub fetching: bool,
    /// The last fetch's failure, when it had one. `value` stays the
    /// last good read — an error never erases it.
    pub error: Option<String>,
}

/// Tuning for a [`Watcher`]. [`Default`] follows the core's own 250 ms →
/// 30 s reconnect backoff and 10 s per-refresh deadlines.
#[derive(Debug, Clone)]
pub struct WatchParams {
    /// The subscription's filters; an unfiltered stream is the default.
    pub subscribe: SubscribeParams,
    /// Document names to watch on this core and every added machine.
    pub documents: Vec<String>,
    /// Document names read only on this core (`machine: None`) — never
    /// fetched on added machines. A `doc` event for one counts only when
    /// the event itself is machine-less.
    pub local_documents: Vec<String>,
    /// First reconnect delay; doubles each failure to `reconnect_max`.
    pub reconnect_initial: Duration,
    /// The backoff ceiling (core's `reconnect_max` is 30 s).
    pub reconnect_max: Duration,
    /// Per-`doc.get` deadline — never the 120 s generic call deadline.
    pub doc_timeout: Duration,
    /// Per-`machine.list`/`terminal.list` deadline.
    pub list_timeout: Duration,
    /// Concurrent document fetches; slow machines fill only their slots.
    pub doc_concurrency: usize,
    /// Resolve the authoritative agent traversal order on the candidate
    /// `View` before receipts ingest, so boundary receipts allocate in the
    /// order the UI will draw. `None` keeps incoming order.
    pub traversal: Option<fn(&View) -> Vec<String>>,
}

impl Default for WatchParams {
    fn default() -> WatchParams {
        WatchParams {
            subscribe: SubscribeParams::default(),
            documents: vec!["herdr-app".into(), "hc-tui".into()],
            local_documents: Vec::new(),
            reconnect_initial: Duration::from_millis(250),
            reconnect_max: Duration::from_secs(30),
            doc_timeout: Duration::from_secs(10),
            list_timeout: Duration::from_secs(10),
            doc_concurrency: 4,
            traversal: None,
        }
    }
}

/// The live watcher: owns the actor task and both connections' lifetime.
/// Read state with [`view`](Self::view); await changes with
/// [`changed_since`](Self::changed_since). Drop or [`stop`](Self::stop)
/// ends everything — connections, ssh children, actor and timers
/// included.
pub struct Watcher {
    view: Arc<Mutex<View>>,
    /// Publish counter the `changed` subscribers observe.
    version_tx: watch::Sender<u64>,
    /// Coalesced refresh requests; full means one is already pending.
    commands: mpsc::Sender<Command>,
    /// The stop latch the actor selects on.
    stop: watch::Sender<bool>,
    /// The actor's handle for `close` to await; kept inside a lock so a
    /// cancelled `close` leaves `abort` (below) able to kill it anyway.
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Kills the actor even after its `JoinHandle` was taken by a
    /// `close` that never finished — the `Drop` safety net.
    abort: tokio::task::AbortHandle,
}

/// Requests the UI can send; coalesced when the queue fills.
#[derive(Debug)]
pub(crate) enum Command {
    /// Refetch watched documents, machines and the static list.
    Refresh,
}

impl Watcher {
    /// Starts watching `endpoint` with real connections: an RPC client
    /// for refreshes and the dedicated subscription. `ssh_program`
    /// overrides the `ssh` name for tests; production passes `None`.
    pub fn connect(
        endpoint: &Endpoint,
        ssh_program: Option<&OsStr>,
        params: WatchParams,
    ) -> Watcher {
        let ssh = ssh_program.map(|s| s.to_os_string());
        let rpc_endpoint = endpoint.clone();
        let rpc_ssh = ssh.clone();
        let rpc: OpenRpc = Arc::new(move || {
            let endpoint = rpc_endpoint.clone();
            let ssh = rpc_ssh.clone();
            Box::pin(async move { Client::connect(&endpoint, ssh.as_deref()).await })
        });
        let sub_endpoint = endpoint.clone();
        let sub: OpenSub = Arc::new(move |subscribe: SubscribeParams| {
            let endpoint = sub_endpoint.clone();
            let ssh = ssh.clone();
            Box::pin(async move {
                match Subscription::connect(&endpoint, ssh.as_deref(), &subscribe).await {
                    Ok(subscription) => SubAttempt::Subscribed(Box::new(subscription)),
                    Err(SubscribeError::Call(CallError::Unsupported { reason, .. })) => {
                        SubAttempt::Unsupported(reason)
                    }
                    Err(e) => SubAttempt::Failed(e),
                }
            })
        });
        Watcher::with_openers(rpc, sub, params)
    }

    /// Starts watching over injected openers — tests hand in fake-core
    /// duplex pairs; every call must build a fresh transport.
    pub fn with_openers(rpc: OpenRpc, sub: OpenSub, params: WatchParams) -> Watcher {
        let view = Arc::new(Mutex::new(View::default()));
        let (version_tx, _) = watch::channel(0u64);
        let (commands, command_rx) = mpsc::channel(4);
        let (stop, stop_rx) = watch::channel(false);
        let task = tokio::spawn(
            actor::Actor::new(
                params,
                Arc::clone(&view),
                version_tx.clone(),
                command_rx,
                stop_rx,
                rpc,
                sub,
            )
            .run(),
        );
        let abort = task.abort_handle();
        Watcher {
            view,
            version_tx,
            commands,
            stop,
            task: Mutex::new(Some(task)),
            abort,
        }
    }

    /// The newest complete state — always consistent, never partially
    /// applied.
    pub fn view(&self) -> View {
        self.view.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// The publish counter [`changed_since`](Self::changed_since)
    /// compares against.
    pub fn version(&self) -> u64 {
        self.view().version
    }

    /// Resolves once a publish with `version > seen` exists — including
    /// one that already landed between reading [`view`](Self::view) and
    /// this call. The cursor makes every publish observable; a fresh
    /// "next change" receiver would silently skip a publish in that gap.
    pub async fn changed_since(&self, seen: u64) {
        let mut rx = self.version_tx.subscribe();
        let _ = rx.wait_for(|version| *version > seen).await;
    }

    /// Asks for a full refresh (documents, machines, static list).
    /// Coalesced: a pending request makes this a no-op.
    pub fn refresh(&self) {
        let _ = self.commands.try_send(Command::Refresh);
    }

    /// Stops reconnects, drops both connections and ends the actor.
    /// Idempotent.
    pub fn stop(&self) {
        self.stop.send_replace(true);
    }

    /// `stop` plus awaiting the actor's end — bounded only by owned
    /// connection teardown, not by peer behavior. The handle is taken
    /// synchronously, so no lock guard crosses the await; a cancelled
    /// `close` drops the local `JoinHandle` (detaching the task) but
    /// `Drop` still has `abort` armed to kill it.
    pub async fn close(self) {
        self.stop();
        let task = self.task.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(task) = task {
            let _ = task.await;
        }
    }
}

impl Drop for Watcher {
    /// Dropping without `close` still stops the actor: the stop latch
    /// fires and `abort` kills the task — even when a cancelled `close`
    /// already took its `JoinHandle`, so no connection or child can
    /// outlive the handle.
    fn drop(&mut self) {
        self.stop();
        self.abort.abort();
    }
}

/// A completed `doc.get`, tagged for staleness checks: the rpc epoch it
/// was sent on and the per-document fetch generation it belongs to.
pub(crate) struct DocDone {
    pub(crate) key: DocKey,
    pub(crate) rpc_epoch: u64,
    pub(crate) fetch_gen: u64,
    pub(crate) result: Result<DocGetResult, CallError>,
}

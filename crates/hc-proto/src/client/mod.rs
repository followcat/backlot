// Derived from herdr-core schema/README.md and crates/herdr-core/src/server.rs (Apache-2.0).

//! The typed concurrent RPC client over one framed connection.
//!
//! `hello` negotiates generation 1 and framed mode; channel-0 calls then run
//! concurrently, correlated by id — a slow remote call cannot stall unrelated
//! ones. Capabilities come from `hello` (refined by `rpc.discover` when the
//! core offers it); a missing method disables only its feature with a reason.
//! The transport's child process, the reader/writer tasks and every pending
//! call are owned here: either task's end — EOF, a wire failure, or a
//! protocol violation — shuts the whole connection down. Calls are never
//! replayed automatically.

mod api;
mod attach;
mod call;
mod error;
mod pump;
mod wire;

pub use attach::{Attach, AttachError, CloseReason, Ended, Output};
pub use error::{CallError, Error};

use crate::OpenRpcDocument;
use crate::codec::{Reader, Writer};
use crate::endpoint::Endpoint;
use crate::hello::{Capabilities, CoreInfo, HelloResult};
use crate::transport::{self, Guard, ReadHalf, WriteHalf};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::sync::atomic::AtomicI64;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Deadline for opening the transport (socket connect or ssh spawn).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Deadline for the `hello` reply.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// Queued-but-unwritten frames. Full applies backpressure to callers.
const OUTGOING_QUEUE: usize = 64;

/// A handle to one negotiated connection. Clone it freely: every clone
/// shares the wire, and the last drop shuts everything down.
pub struct Client {
    shared: Arc<Shared>,
}

impl Clone for Client {
    fn clone(&self) -> Client {
        Client {
            shared: Arc::clone(&self.shared),
        }
    }
}

struct Shared {
    /// The validated `hello` result (generation 1, framed).
    hello: Arc<HelloResult>,
    /// Wire state the calls and tasks share.
    wire: Arc<call::Wire>,
    guard: Mutex<Option<Guard>>,
    /// Reader, writer, and discovery task handles — aborted before they are
    /// awaited so a cancelled `close` cannot leak actors.
    tasks: Mutex<Vec<JoinHandle<()>>>,
    /// The `rpc.discover` document, when the core offered it and answered.
    discovery: Arc<Mutex<Option<OpenRpcDocument>>>,
    /// Method names validated out of the discovery document. `None` means
    /// capabilities come from `hello` alone.
    discovery_methods: Arc<Mutex<Option<BTreeSet<String>>>>,
    /// Why discovery has no document: not offered, unusable, or the call's
    /// failure.
    discovery_note: Arc<Mutex<Option<String>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

impl Shared {
    /// One shutdown for every end-of-wire reason — reader EOF/error,
    /// writer failure, a protocol violation, `close`, last drop. Fails all
    /// pending calls (atomically marking the wire dead), aborts the sibling
    /// tasks so both stream halves drop (the peer sees stdin EOF), and
    /// hands the backend child to a bounded graceful close.
    fn shutdown(&self, reason: Option<String>) {
        pump::fail(&self.wire.pending, reason);
        for task in lock(&self.tasks).iter() {
            task.abort();
        }
        if let Some(guard) = lock(&self.guard).take()
            && let Ok(rt) = tokio::runtime::Handle::try_current()
        {
            rt.spawn(async move {
                let _ = guard.close().await;
            });
        }
    }
}

impl Drop for Shared {
    /// The last handle is gone: fail pending calls, stop the tasks and drop
    /// the guard (which kills and reaps an ssh child). Runs synchronously so
    /// waiters wake immediately. Background tasks hold `Arc<Wire>` cells,
    /// never `Shared`, so this runs even while one is still finishing.
    fn drop(&mut self) {
        pump::fail(&self.wire.pending, None);
        for task in self
            .tasks
            .get_mut()
            .unwrap_or_else(|p| p.into_inner())
            .drain(..)
        {
            task.abort();
        }
        // Dropping the guard kills and reaps an ssh child (kill_on_drop).
        drop(
            self.guard
                .get_mut()
                .unwrap_or_else(|p| p.into_inner())
                .take(),
        );
    }
}

impl Client {
    /// Opens `endpoint` and negotiates the framed RPC wire. `ssh_program`
    /// overrides the `ssh` name for tests; production passes `None`.
    pub async fn connect(
        endpoint: &Endpoint,
        ssh_program: Option<&OsStr>,
    ) -> Result<Client, Error> {
        let conn = tokio::time::timeout(CONNECT_TIMEOUT, transport::connect(endpoint, ssh_program))
            .await
            .map_err(|_| Error::Timeout("connect"))?
            .map_err(Error::Transport)?;
        let (reader, writer, guard) = conn.split();
        Client::negotiated(reader, writer, Some(guard)).await
    }

    /// Negotiates over an injected byte stream — tests use in-memory wires or
    /// scratch sockets. `guard` owns the backend's lifetime when there is one.
    pub async fn open(
        read: ReadHalf,
        write: WriteHalf,
        guard: Option<Guard>,
    ) -> Result<Client, Error> {
        Client::negotiated(Reader::new(read), Writer::new(write), guard).await
    }

    async fn negotiated(
        mut reader: Reader<ReadHalf>,
        mut writer: Writer<WriteHalf>,
        guard: Option<Guard>,
    ) -> Result<Client, Error> {
        let hello =
            match tokio::time::timeout(HELLO_TIMEOUT, wire::negotiate(&mut reader, &mut writer))
                .await
            {
                Ok(Ok(hello)) => hello,
                Ok(Err(e)) => return Err(Client::hello_failed(e, reader, writer, guard).await),
                Err(_) => {
                    return Err(Client::hello_failed(
                        Error::Timeout("hello"),
                        reader,
                        writer,
                        guard,
                    )
                    .await);
                }
            };
        let client = Client::spawn(reader, writer, guard, hello);
        // Discovery is optional enrichment: it never blocks `open`, its own
        // call is bounded by DISCOVERY_TIMEOUT, and the task captures wire
        // cells — not `Shared` — so dropping the last handle still cleans
        // up immediately.
        let task = tokio::spawn({
            let shared = Arc::clone(&client.shared);
            let hello = Arc::clone(&shared.hello);
            let wire = Arc::clone(&shared.wire);
            let cells = (
                Arc::clone(&shared.discovery),
                Arc::clone(&shared.discovery_methods),
                Arc::clone(&shared.discovery_note),
            );
            async move { call::discover(&hello, &wire, &cells).await }
        });
        lock(&client.shared.tasks).push(task);
        Ok(client)
    }

    /// A negotiation failure with a backend behind it. The codec halves are
    /// dropped first — otherwise the child never sees stdin EOF and
    /// `guard.close()` can only end it at the kill timeout. Then the child's
    /// own exit status and stderr tail are kept when the peer actually died
    /// (auth refused, no hc there). A real protocol answer — an error reply,
    /// a generation mismatch — is never replaced by cleanup diagnostics.
    async fn hello_failed(
        err: Error,
        reader: Reader<ReadHalf>,
        writer: Writer<WriteHalf>,
        guard: Option<Guard>,
    ) -> Error {
        drop(reader);
        drop(writer);
        let Some(guard) = guard else { return err };
        let closed = guard.close().await;
        match err {
            Error::Closed | Error::Codec(_) => match closed {
                Err(transport) => Error::Transport(transport),
                Ok(_) => err,
            },
            _ => err,
        }
    }

    fn spawn(
        reader: Reader<ReadHalf>,
        writer: Writer<WriteHalf>,
        guard: Option<Guard>,
        hello: HelloResult,
    ) -> Client {
        let (tx, rx) = mpsc::channel(OUTGOING_QUEUE);
        let shared = Arc::new(Shared {
            hello: Arc::new(hello),
            wire: Arc::new(call::Wire {
                tx,
                pending: pump::Pending::default(),
                next_id: AtomicI64::new(1),
            }),
            guard: Mutex::new(guard),
            tasks: Mutex::new(Vec::new()),
            discovery: Arc::new(Mutex::new(None)),
            discovery_methods: Arc::new(Mutex::new(None)),
            discovery_note: Arc::new(Mutex::new(None)),
        });
        let closer = |shared: &Arc<Shared>| {
            let weak = Arc::downgrade(shared);
            move || {
                if let Some(shared) = weak.upgrade() {
                    shared.shutdown(None);
                }
            }
        };
        let reader_task = tokio::spawn(pump::read_loop(
            reader,
            shared.wire.pending.clone(),
            closer(&shared),
        ));
        let writer_task = tokio::spawn(pump::write_loop(writer, rx, closer(&shared)));
        *lock(&shared.tasks) = vec![reader_task, writer_task];
        Client { shared }
    }

    /// The validated handshake: generation, identity, methods, features.
    pub fn hello(&self) -> &HelloResult {
        &self.shared.hello
    }

    /// A checked capability view over the handshake.
    pub fn capabilities(&self) -> Capabilities<'_> {
        Capabilities::new(&self.shared.hello).expect("hello validated at connect")
    }

    /// The core's identity, for the UI to title the connection.
    pub fn core(&self) -> &CoreInfo {
        &self.shared.hello.core
    }

    /// The `rpc.discover` document when the core offered and answered it.
    /// Opaque; clone when the caller needs it.
    pub fn discovery(&self) -> Option<OpenRpcDocument> {
        lock(&self.shared.discovery).clone()
    }

    /// Why [`discovery`](Self::discovery) is empty — a diagnostic limitation,
    /// never a crash.
    pub fn discovery_note(&self) -> Option<String> {
        lock(&self.shared.discovery_note).clone()
    }

    /// Calls currently waiting for a reply.
    pub fn in_flight(&self) -> usize {
        self.shared.wire.pending.len()
    }

    /// Whether the wire is known dead.
    pub fn is_closed(&self) -> bool {
        self.shared.wire.pending.is_dead()
    }

    /// Resolves when the wire dies — EOF, a wire failure, a protocol
    /// violation or the last drop — for event-driven recovery instead of
    /// `is_closed` polling. Latched: an already-dead wire resolves
    /// immediately, and a `closed` subscribed before the end still fires.
    pub async fn closed(&self) {
        let mut dead = self.shared.wire.pending.dead_rx();
        let _ = dead.wait_for(|dead| *dead).await;
    }

    /// Orderly shutdown: pending calls fail, the backend guard is taken into
    /// owned cleanup *before* the first await (a cancelled close can never
    /// leak a half-live task — aborted tasks die when their handles drop,
    /// and the owned guard still kills the child), the write half closes so
    /// a remote `hc connect` sees stdin EOF, then the guard waits for or
    /// kills the child and reports its status and stderr tail. Works even
    /// while the peer is not reading.
    pub async fn close(self) -> Result<transport::Closed, transport::Error> {
        let shared = Arc::clone(&self.shared);
        pump::fail(&shared.wire.pending, None);
        let guard = lock(&shared.guard).take();
        let tasks = lock(&shared.tasks).drain(..).collect::<Vec<_>>();
        for task in &tasks {
            task.abort();
        }
        for task in tasks {
            let _ = task.await;
        }
        match guard {
            Some(guard) => guard.close().await,
            None => Ok(transport::Closed::default()),
        }
    }
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("core", &self.shared.hello.core.name)
            .field("closed", &self.is_closed())
            .finish_non_exhaustive()
    }
}

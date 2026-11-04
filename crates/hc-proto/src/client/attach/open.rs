// Derived from herdr-core crates/herdr-core/src/server.rs and viewers.rs (Apache-2.0).

//! Opening an attach: the negotiated hello, the `terminal.attach` call on
//! channel 0, result validation, and the shared cleanup when any of that
//! fails — tasks always aborted and awaited so both halves drop and the
//! child sees stdin EOF, then the backend closes for its real status and
//! stderr tail.

use super::error::{AttachError, with_transport_diagnostics};
use super::pump::{self, CTRL_QUEUE, DataQueue};
use super::stream::Stream;
use super::{Attach, Shared};
use crate::client::error::{CallError, Error};
use crate::client::{call, lock};
use crate::codec::{Reader, Writer};
use crate::endpoint::Endpoint;
use crate::hello::{Capabilities, feature};
use crate::method;
use crate::terminal::{AttachParams, AttachResult};
use crate::transport::{self, Guard, ReadHalf, WriteHalf};
use std::ffi::OsStr;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Deadline for the `terminal.attach` reply (it includes first paint).
const ATTACH_TIMEOUT: Duration = Duration::from_secs(30);

/// Field-level pieces while the attach call is in flight — the tasks are
/// already running before the result names the channel.
///
/// `WireCells` is *armed* by default: dropping it — for example when the
/// `open`/`connect` future is cancelled while the attach RPC is
/// unanswered — synchronously fails pending calls, ends the stream,
/// aborts both tasks and drops the guard, which kills and reaps an owned
/// ssh child. `commit` disarms it once the result is installed and the
/// cells move into `Shared`, which owns cleanup from then on.
pub(super) struct WireCells {
    pub(super) wire: Arc<call::Wire>,
    pub(super) stream: Arc<Stream>,
    pub(super) data: Arc<DataQueue>,
    pub(super) guard: Arc<Mutex<Option<Guard>>>,
    pub(super) tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
    /// `true` once ownership moved to `Shared`.
    committed: AtomicBool,
}

impl WireCells {
    /// The attach succeeded: `Shared` owns cleanup now, drop is a no-op.
    fn commit(&self) {
        self.committed.store(true, Ordering::Relaxed);
    }
}

impl Drop for WireCells {
    /// Armed cleanup for a cancelled/failed open: same synchronous sequence
    /// as `Attach::drop` — fail, end, abort, take+drop the guard (kill
    /// and reap the child).
    fn drop(&mut self) {
        if self.committed.load(Ordering::Relaxed) {
            return;
        }
        crate::client::pump::fail(&self.wire.pending, None);
        self.stream.fail(None);
        self.data.close();
        for task in lock(&self.tasks).drain(..) {
            task.abort();
        }
        drop(lock(&self.guard).take());
    }
}

/// The end-of-wire callback every task gets: field-level `Arc`s only, so a
/// dropped `Attach` never keeps tasks — or the child — alive. Fails pending
/// calls, ends the stream, aborts the siblings so both halves drop (the
/// peer sees stdin EOF) and hands the backend to a bounded close.
fn shutdown_fn(cells: &WireCells) -> impl FnOnce() + Send + 'static {
    let pending = cells.wire.pending.clone();
    let stream = Arc::clone(&cells.stream);
    let data = Arc::clone(&cells.data);
    let tasks = Arc::clone(&cells.tasks);
    let guard = Arc::clone(&cells.guard);
    move || {
        crate::client::pump::fail(&pending, None);
        stream.fail(None);
        data.close();
        for task in lock(&tasks).iter() {
            task.abort();
        }
        if let Some(guard) = lock(&guard).take()
            && let Ok(rt) = tokio::runtime::Handle::try_current()
        {
            rt.spawn(async move {
                let _ = guard.close().await;
            });
        }
    }
}

impl Attach {
    /// Opens `endpoint`, negotiates the framed wire and issues
    /// `terminal.attach`. `ssh_program` overrides the `ssh` name for
    /// tests; production passes `None`.
    pub async fn connect(
        endpoint: &Endpoint,
        ssh_program: Option<&OsStr>,
        params: &AttachParams,
    ) -> Result<Attach, AttachError> {
        let conn = tokio::time::timeout(
            super::super::CONNECT_TIMEOUT,
            transport::connect(endpoint, ssh_program),
        )
        .await
        .map_err(|_| AttachError::Open(Error::Timeout("connect")))?
        .map_err(Error::Transport)?;
        let (reader, writer, guard) = conn.split();
        Attach::negotiated(reader, writer, Some(guard), params).await
    }

    /// Attaches over an injected byte stream — tests use in-memory wires.
    /// `guard` owns the backend's lifetime when there is one.
    pub async fn open(
        read: ReadHalf,
        write: WriteHalf,
        guard: Option<Guard>,
        params: &AttachParams,
    ) -> Result<Attach, AttachError> {
        Attach::negotiated(Reader::new(read), Writer::new(write), guard, params).await
    }

    async fn negotiated(
        mut reader: Reader<ReadHalf>,
        mut writer: Writer<WriteHalf>,
        guard: Option<Guard>,
        params: &AttachParams,
    ) -> Result<Attach, AttachError> {
        let hello = match tokio::time::timeout(
            super::super::HELLO_TIMEOUT,
            crate::client::wire::negotiate(&mut reader, &mut writer),
        )
        .await
        {
            Ok(Ok(hello)) => hello,
            Ok(Err(e)) => {
                return Err(open_failed(AttachError::Open(e), reader, writer, guard).await);
            }
            Err(_) => {
                return Err(open_failed(
                    AttachError::Open(Error::Timeout("hello")),
                    reader,
                    writer,
                    guard,
                )
                .await);
            }
        };
        let caps = match Capabilities::new(&hello) {
            Ok(caps) => caps,
            Err(e) => {
                return Err(open_failed(
                    AttachError::Open(Error::Generation(e)),
                    reader,
                    writer,
                    guard,
                )
                .await);
            }
        };
        if !caps.has_method(method::TERMINAL_ATTACH) {
            return Err(open_failed(
                AttachError::Call(CallError::Unsupported {
                    method: method::TERMINAL_ATTACH.into(),
                    reason: format!("core {} does not offer it", hello.core.name),
                }),
                reader,
                writer,
                guard,
            )
            .await);
        }
        let automatic_input = caps.has_feature(feature::AUTOMATIC_INPUT);
        let observe = params.observe;
        // Tasks spawn before the call so early frames — including a first
        // paint in the same read — are held by channel until install.
        let cells = Attach::spawn(reader, writer, guard);
        let params = match serde_json::to_value(params) {
            Ok(params) => params,
            Err(e) => {
                return Err(attach_failed(AttachError::Call(CallError::Shape(e)), &cells).await);
            }
        };
        let result = match call::wire_call(
            &cells.wire,
            method::TERMINAL_ATTACH,
            Some(params),
            ATTACH_TIMEOUT,
        )
        .await
        {
            Ok(result) => result,
            Err(e) => return Err(attach_failed(AttachError::Call(e), &cells).await),
        };
        let result: AttachResult = match serde_json::from_value(result) {
            Ok(result) => result,
            Err(e) => {
                return Err(attach_failed(AttachError::Call(CallError::Shape(e)), &cells).await);
            }
        };
        if result.channel == 0 {
            return Err(attach_failed(
                AttachError::Protocol("attach returned channel 0".into()),
                &cells,
            )
            .await);
        }
        if let Err(why) = cells.stream.install(result.channel) {
            return Err(attach_failed(AttachError::Protocol(why), &cells).await);
        }
        cells.commit();
        Ok(Attach {
            shared: Arc::new(Shared {
                hello: Arc::new(hello),
                terminal: result.terminal,
                channel: result.channel,
                observe,
                resize_notifications: result.resize_notifications,
                automatic_input: automatic_input && result.automatic_input,
                wire: Arc::clone(&cells.wire),
                stream: Arc::clone(&cells.stream),
                data: Arc::clone(&cells.data),
                guard: Arc::clone(&cells.guard),
                tasks: Arc::clone(&cells.tasks),
            }),
        })
    }

    /// Spawns the demux reader and biased writer over the freshly
    /// negotiated halves. The stream's channel is installed once the
    /// attach reply lands.
    fn spawn(
        reader: Reader<ReadHalf>,
        writer: Writer<WriteHalf>,
        guard: Option<Guard>,
    ) -> WireCells {
        let (ctrl, ctrl_rx) = mpsc::channel(CTRL_QUEUE);
        let cells = WireCells {
            wire: Arc::new(call::Wire {
                tx: ctrl,
                pending: crate::client::pump::Pending::default(),
                next_id: AtomicI64::new(1),
            }),
            stream: Arc::new(Stream::new()),
            data: Arc::new(DataQueue::new()),
            guard: Arc::new(Mutex::new(guard)),
            tasks: Arc::new(Mutex::new(Vec::new())),
            committed: AtomicBool::new(false),
        };
        let reader_task = tokio::spawn(pump::read_loop(
            reader,
            cells.wire.pending.clone(),
            Arc::clone(&cells.stream),
            Arc::clone(&cells.data),
            shutdown_fn(&cells),
        ));
        let writer_task = tokio::spawn(pump::write_loop(
            writer,
            ctrl_rx,
            Arc::clone(&cells.data),
            Arc::clone(&cells.stream),
            shutdown_fn(&cells),
        ));
        *lock(&cells.tasks) = vec![reader_task, writer_task];
        cells
    }
}

/// A failure before the tasks spawn: the codec halves drop first so the
/// child sees stdin EOF promptly, then the backend's own status and
/// stderr tail replace a bare `Closed`/codec error — never a real
/// protocol answer.
async fn open_failed(
    err: AttachError,
    reader: Reader<ReadHalf>,
    writer: Writer<WriteHalf>,
    guard: Option<Guard>,
) -> AttachError {
    drop(reader);
    drop(writer);
    let Some(guard) = guard else { return err };
    with_transport_diagnostics(err, guard.close().await)
}

/// A failure after the tasks spawned (the attach call or its result).
/// Tasks are aborted and awaited so both halves drop — the child's stdin
/// EOFs — then the backend is closed for its status and stderr tail.
/// Only a bare `Closed` gets transport diagnostics; structured replies
/// (a `finished` terminal) are never masked.
async fn attach_failed(err: AttachError, cells: &WireCells) -> AttachError {
    crate::client::pump::fail(&cells.wire.pending, None);
    cells.stream.fail(None);
    cells.data.close();
    let guard = lock(&cells.guard).take();
    let tasks = lock(&cells.tasks).drain(..).collect::<Vec<_>>();
    for task in &tasks {
        task.abort();
    }
    for task in tasks {
        let _ = task.await;
    }
    cells.commit(); // this path owns cleanup; the armed Drop is discharged
    let Some(guard) = guard else { return err };
    with_transport_diagnostics(err, guard.close().await)
}

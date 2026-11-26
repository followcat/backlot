// Derived from herdr-core crates/herdr-core/src/server.rs and viewers.rs (Apache-2.0).

//! The shared framed connection many `Attach` handles ride: one transport,
//! one `hello`, channel-0 attach calls, the channel table and one
//! biased-but-fair writer. [`AttachConn`] is the lazy, single-flight
//! registry a client holds per endpoint; [`Conn`] is one live connection —
//! its claims keep it alive, its `died` is the one shared shutdown every
//! attach on it ends through.

use super::calls::AttachCalls;
use super::diag::open_failed;
use super::error::AttachError;
use super::mux::{self, Cells};
use super::pump::{CTRL_QUEUE, DataQueue};
use super::registry::AttachGauges;
use super::stream::Stream;
use super::table::{Channels, Install};
use super::{Attach, Shared};
use crate::client::call;
use crate::client::error::{CallError, Error};
use crate::client::{lock, pump, wire};
use crate::codec::{Reader, Writer};
use crate::endpoint::Endpoint;
use crate::hello::{Capabilities, HelloResult, feature};
use crate::method;
use crate::terminal::{AttachParams, AttachResult};
use crate::transport::{self, Guard, ReadHalf, WriteHalf};
use std::ffi::OsStr;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{Notify, mpsc};
use tokio::task::JoinHandle;

/// One live shared connection: the negotiated `hello` every attach
/// shares, the channel-0 control wire, the channel table and the two
/// background halves. `dead` latches at `died`'s start; the counted
/// claims — opens in flight plus installed attaches — run the shared
/// shutdown when the last one leaves.
pub(super) struct Conn {
    /// The validated `hello` every attach shares.
    hello: Arc<HelloResult>,
    /// Channel-0 control queue + the wire's dead latch.
    pub(super) wire: Arc<call::Wire>,
    /// The channel table the reader routes through.
    channels: Arc<Mutex<Channels>>,
    /// Attach calls in flight — ids their sent requests still answer.
    pub(super) attach_calls: Arc<AttachCalls>,
    /// Wakes the shared writer for any channel's work.
    work: Arc<Notify>,
    /// Live claims — opens in flight plus installed attaches. The last
    /// one's `leave` shuts the connection down.
    count: Mutex<usize>,
    /// Latched once `died` ran — the wire's dead state.
    dead: AtomicBool,
    /// The backend child — owned here, never per attach.
    guard: Mutex<Option<Guard>>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    /// The spawned backend close, once `died` started it.
    pub(super) close_task: Mutex<Option<JoinHandle<Result<transport::Closed, transport::Error>>>>,
}

/// One claim keeping the connection alive: an open in flight or an
/// installed attach. `leave` consumes it cleanly; dropping before
/// `commit` means an in-flight attach was cancelled.
pub(super) struct Ticket {
    conn: Option<Arc<Conn>>,
    cancel_on_drop: bool,
}

impl Ticket {
    /// Marks this ticket past cancellation — subsequent drops use graceful `leave`.
    pub(super) fn commit(&mut self) {
        self.cancel_on_drop = false;
    }

    /// Ends this claim: `true` when it was the last and the connection
    /// just shut down.
    pub(super) fn leave(mut self) -> bool {
        self.cancel_on_drop = false;
        match self.conn.take() {
            Some(conn) => conn.leave(),
            None => false,
        }
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            if self.cancel_on_drop {
                conn.leave_cancelled();
            } else {
                conn.leave();
            }
        }
    }
}

impl Conn {
    /// Opens `endpoint` and negotiates the framed wire. `ssh_program`
    /// overrides the `ssh` name for tests; production passes `None`.
    pub(super) async fn connect(
        endpoint: &Endpoint,
        ssh_program: Option<&OsStr>,
        stats: &Arc<crate::stats::ConnStats>,
    ) -> Result<Arc<Conn>, AttachError> {
        let conn = tokio::time::timeout(
            super::super::CONNECT_TIMEOUT,
            transport::connect(endpoint, ssh_program),
        )
        .await
        .map_err(|_| AttachError::Open(Error::Timeout("connect")))?
        .map_err(Error::Transport)?;
        let (reader, writer, guard) = conn.counted(stats).split();
        let conn = Conn::negotiate(reader, writer, Some(guard)).await?;
        stats.connected();
        Ok(conn)
    }

    /// What this connection holds right now, for diagnostics.
    pub(super) fn gauges(&self) -> AttachGauges {
        let (live, held, dead) = lock(&self.channels).census();
        AttachGauges {
            calls_in_flight: self.attach_calls.len(),
            channels_live: live,
            channels_held: held,
            channels_dead: dead,
        }
    }

    /// Negotiates `hello` over an injected byte stream — tests use
    /// in-memory wires — then spawns the shared halves.
    pub(super) async fn negotiate(
        mut reader: Reader<ReadHalf>,
        mut writer: Writer<WriteHalf>,
        guard: Option<Guard>,
    ) -> Result<Arc<Conn>, AttachError> {
        let hello = match tokio::time::timeout(
            super::super::HELLO_TIMEOUT,
            wire::negotiate(&mut reader, &mut writer),
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
        Ok(Conn::spawn(reader, writer, guard, hello))
    }

    /// Spawns the demux reader and the biased-but-fair writer over the
    /// freshly negotiated halves.
    fn spawn(
        reader: Reader<ReadHalf>,
        writer: Writer<WriteHalf>,
        guard: Option<Guard>,
        hello: HelloResult,
    ) -> Arc<Conn> {
        let (ctrl, ctrl_rx) = mpsc::channel(CTRL_QUEUE);
        let conn = Arc::new(Conn {
            hello: Arc::new(hello),
            wire: Arc::new(call::Wire {
                tx: ctrl,
                pending: pump::Pending::default(),
                next_id: AtomicI64::new(1),
                probing: AtomicBool::new(false),
            }),
            channels: Arc::new(Mutex::new(Channels::default())),
            attach_calls: Arc::new(AttachCalls::default()),
            work: Arc::new(Notify::new()),
            count: Mutex::new(0),
            dead: AtomicBool::new(false),
            guard: Mutex::new(guard),
            tasks: Mutex::new(Vec::new()),
            close_task: Mutex::new(None),
        });
        let on_end = |conn: &Arc<Conn>| {
            let weak = Arc::downgrade(conn);
            move |reason: Option<String>| {
                if let Some(conn) = weak.upgrade() {
                    conn.died(reason, true);
                }
            }
        };
        let cells = Cells {
            wire: Arc::clone(&conn.wire),
            channels: Arc::clone(&conn.channels),
            attach_calls: Arc::clone(&conn.attach_calls),
        };
        let reader_task = tokio::spawn(mux::read_loop(reader, cells, on_end(&conn)));
        let writer_end = {
            let weak = Arc::downgrade(&conn);
            move || {
                if let Some(conn) = weak.upgrade() {
                    conn.died(None, true);
                }
            }
        };
        let writer_task = tokio::spawn(mux::write_loop(
            writer,
            ctrl_rx,
            Arc::clone(&conn.channels),
            Arc::clone(&conn.work),
            writer_end,
        ));
        *lock(&conn.tasks) = vec![reader_task, writer_task];
        conn
    }

    /// Whether the connection is dead — latched at `died`'s start.
    pub(super) fn dead(&self) -> bool {
        self.dead.load(Ordering::SeqCst)
    }

    /// The channel table's size — test access for the dead-slot bound.
    pub(super) fn channel_map_len(&self) -> usize {
        lock(&self.channels).len()
    }

    /// Attach calls currently registered — test access.
    pub(super) fn attach_call_count(&self) -> usize {
        self.attach_calls.len()
    }

    /// One claim on the connection's life: `Closed` once dead, checked
    /// under the count lock so a claim can never slip in after the last
    /// `leave` started shutdown.
    fn claim(self: &Arc<Self>) -> Result<Ticket, AttachError> {
        let mut count = lock(&self.count);
        if self.dead() {
            return Err(AttachError::Call(CallError::Closed));
        }
        *count += 1;
        Ok(Ticket {
            conn: Some(Arc::clone(self)),
            cancel_on_drop: true,
        })
    }

    fn release_claim(&self) -> bool {
        let mut count = lock(&self.count);
        *count -= 1;
        *count == 0
    }

    /// A claim ended: `true` — and shared shutdown — when it was the last.
    fn leave(&self) -> bool {
        let last = self.release_claim();
        if last {
            self.died(None, false);
        }
        last
    }

    /// A cancelled in-flight open ended: abort immediately when it was the last claim
    /// rather than sleeping out `CLOSE_TIMEOUT` on an abandoned child.
    fn leave_cancelled(&self) {
        if self.release_claim() {
            self.abort();
        }
    }

    async fn attach_inner(
        &self,
        params: &AttachParams,
    ) -> Result<(AttachResult, bool, bool, bool), AttachError> {
        let caps = Capabilities::new(&self.hello).expect("hello validated at connect");
        if !caps.has_method(method::TERMINAL_ATTACH) {
            return Err(AttachError::Call(CallError::Unsupported {
                method: method::TERMINAL_ATTACH.into(),
                reason: format!("core {} does not offer it", self.hello.core.name),
            }));
        }
        let automatic_input = caps.has_feature(feature::AUTOMATIC_INPUT);
        let observe = params.observe;
        let quiet_human_release = params.quiet_human_release
            && !observe
            && caps.has_feature(feature::QUIET_HUMAN_RELEASE);
        let mut params = params.clone();
        params.quiet_human_release = quiet_human_release;
        let params = serde_json::to_value(params).map_err(CallError::Shape)?;
        let result = self.attach_call(params).await?;
        // The core allocated the reply's channel already: read it
        // leniently so a failed `AttachResult` shape still owes that
        // channel a CLOSE — a bare error would leak the viewer for the
        // shared connection's whole life.
        let channel = mux::reply_channel(&result);
        let result: AttachResult = match serde_json::from_value(result) {
            Ok(result) => result,
            Err(err) => {
                if let Some(channel) = channel {
                    mux::queue_close(&self.wire, channel);
                }
                return Err(AttachError::Call(CallError::Shape(err)));
            }
        };
        if result.channel == 0 {
            return Err(AttachError::Protocol("attach returned channel 0".into()));
        }
        Ok((result, observe, automatic_input, quiet_human_release))
    }

    /// `terminal.attach` on the shared wire. The claim keeps the
    /// connection alive across the call — a last-detach racing the open
    /// cannot kill the wire under it.
    pub(super) async fn attach(
        self: &Arc<Self>,
        params: &AttachParams,
    ) -> Result<Attach, AttachError> {
        let mut ticket = self.claim()?;
        let (result, observe, automatic_input, quiet_human_release) =
            match self.attach_inner(params).await {
                Ok(ok) => ok,
                Err(err) => {
                    ticket.commit();
                    return Err(err);
                }
            };
        ticket.commit();
        let stream = Arc::new(Stream::new(Arc::clone(&self.work)));
        let data = Arc::new(DataQueue::new(Arc::clone(&self.work)));
        match lock(&self.channels).install(result.channel, Arc::clone(&stream), Arc::clone(&data)) {
            Install::Ok => self.work.notify_one(),
            Install::Dead => {
                // The connection died between the reply and install —
                // hand out the attach already failed, the same
                // dead-on-arrival shape a dedicated open produced when
                // its wire died mid-attach.
                stream.fail(None);
                data.close();
            }
            Install::Violation(why) => {
                mux::queue_close(&self.wire, result.channel);
                return Err(AttachError::Protocol(why));
            }
        }
        Ok(Attach {
            shared: Arc::new(Shared {
                hello: Arc::clone(&self.hello),
                terminal: result.terminal,
                channel: result.channel,
                observe,
                resize_notifications: result.resize_notifications,
                automatic_input: automatic_input && result.automatic_input,
                quiet_human_release: quiet_human_release && result.quiet_human_release,
                wire: Arc::clone(&self.wire),
                stream,
                data,
                conn: Arc::clone(self),
                ticket: Mutex::new(Some(ticket)),
            }),
        })
    }

    /// Client-side channel teardown: the attach's own end already failed
    /// its stream — this just swallows the channel's late frames.
    pub(super) fn end_channel(&self, channel: u32) {
        lock(&self.channels).end(channel);
    }

    /// The connection's own end — a dead transport, a channel-0 protocol
    /// violation, or the last claim leaving. `eof` picks the backend's
    /// exit grace: transport ends (`true`) get the prompt EOF close — the
    /// peer already stopped speaking, so a hung child is killed inside
    /// the grace rather than blocking `diagnose` — while the last claim's
    /// clean leave (`false`) keeps the full graceful close. Idempotent:
    /// fails pending calls and in-flight attach opens, fails every live
    /// channel with the same reason, aborts both halves so the streams
    /// drop (the peer sees stdin EOF) and hands the backend to the
    /// spawned close.
    fn fail_and_stop(&self, reason: Option<String>) {
        pump::fail(&self.wire.pending, reason.clone());
        self.attach_calls.fail_all(reason.clone());
        lock(&self.channels).fail_all(reason);
        for task in lock(&self.tasks).iter() {
            task.abort();
        }
    }

    /// Aborts an abandoned connection without graceful close delay, dropping `guard` directly.
    fn abort(&self) {
        self.dead.store(true, Ordering::SeqCst);
        self.fail_and_stop(None);
        if let Some(task) = lock(&self.close_task).take() {
            task.abort();
        }
        drop(lock(&self.guard).take());
    }

    pub(super) fn died(&self, reason: Option<String>, eof: bool) {
        if self.dead.swap(true, Ordering::SeqCst) {
            return;
        }
        self.fail_and_stop(reason);
        if let Some(guard) = lock(&self.guard).take()
            && let Ok(rt) = tokio::runtime::Handle::try_current()
        {
            *lock(&self.close_task) = Some(rt.spawn(async move {
                if eof {
                    guard.close_eof().await
                } else {
                    guard.close().await
                }
            }));
        }
    }
}

impl Drop for Conn {
    /// The registry and every claim are gone: fail, end, abort, take the
    /// guard — which kills and reaps an ssh child.
    fn drop(&mut self) {
        pump::fail(&self.wire.pending, None);
        self.attach_calls.fail_all(None);
        lock(&self.channels).fail_all(None);
        for task in self
            .tasks
            .get_mut()
            .unwrap_or_else(|p| p.into_inner())
            .drain(..)
        {
            task.abort();
        }
        drop(
            self.guard
                .get_mut()
                .unwrap_or_else(|p| p.into_inner())
                .take(),
        );
    }
}

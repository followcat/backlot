// Derived from herdr-core crates/herdr-core/src/server.rs and viewers.rs (Apache-2.0).

//! A terminal attach: `terminal.attach` on its own negotiated connection,
//! then framed stream I/O on the channel it returns.
//!
//! The attach rides the same ownership rules as the RPC client — one
//! shutdown for every end reason, a backend child killed and reaped when
//! the handle drops or a close is cancelled. The reader demultiplexes:
//! channel 0 keeps RPC replies (the attach call itself), stream channels
//! feed an ordered bounded backlog the consumer drains with `recv` —
//! a stalled consumer can never hide a CLOSE because the reader never
//! waits on it. `consume` is the only credit operation and it is
//! synchronous, so a cancelled caller can never lose or duplicate credit
//! bookkeeping while the stream lives.

mod error;
mod open;
mod output;
mod pump;
mod stream;

pub use error::{AttachError, Ended};
pub use output::{CloseReason, Output};

use super::error::CallError;
use super::{call, lock};
use crate::frame::{Frame, Kind};
use crate::hello::HelloResult;
use crate::terminal::{Display, Terminal};
use crate::transport::{self, Guard};
use pump::DataQueue;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use stream::Stream;
use tokio::task::JoinHandle;

/// How long `detach` waits for the core's CLOSE ack before closing anyway.
const DETACH_TIMEOUT: Duration = Duration::from_secs(10);
/// One input frame's payload: comfortably below the core's chunking.
const INPUT_CHUNK: usize = 16 * 1024;

/// One attached terminal: events via `recv`, bytes out via `send_input`,
/// credit via `consume`, an orderly end via `detach`. Dropping it ends the
/// stream (no CLOSE handshake) and closes the owned connection.
pub struct Attach {
    shared: Arc<Shared>,
}

pub(super) struct Shared {
    /// The validated `hello` result.
    hello: Arc<HelloResult>,
    /// The attached terminal's record — the authoritative initial grid.
    terminal: Terminal,
    /// The negotiated stream channel (never 0).
    channel: u32,
    observe: bool,
    /// `true` only when the whole path confirms RESIZE-before-paint.
    resize_notifications: bool,
    /// `true` only when both `hello` and the reply accept AUTOMATIC_INPUT.
    automatic_input: bool,
    /// Control queue + pending table for channel-0 calls.
    wire: Arc<call::Wire>,
    /// Inbound routing, backlog and credit.
    stream: Arc<Stream>,
    /// Queued stream input.
    data: Arc<DataQueue>,
    /// `Arc`s so the tasks' end-of-wire closures own cells, never `Shared`.
    guard: Arc<Mutex<Option<Guard>>>,
    tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl Attach {
    /// The attached terminal's record — the authoritative initial grid.
    pub fn terminal(&self) -> &Terminal {
        &self.shared.terminal
    }

    /// The negotiated stream channel.
    pub fn channel(&self) -> u32 {
        self.shared.channel
    }

    /// The validated handshake, for the core's identity and features.
    pub fn hello(&self) -> &HelloResult {
        &self.shared.hello
    }

    /// `true` when this attach is an observer — no input, resize, focus
    /// or facts can be sent.
    pub fn observe(&self) -> bool {
        self.shared.observe
    }

    /// `true` only when the entire path sends RESIZE before replacement
    /// paints — a confirmed `resize_notifications` result.
    pub fn resize_notifications(&self) -> bool {
        self.shared.resize_notifications
    }

    /// `true` when both `hello.features` and the attach reply confirmed
    /// AUTOMATIC_INPUT — only then does `reply` use kind 6.
    pub fn automatic_input(&self) -> bool {
        self.shared.automatic_input
    }

    /// DATA bytes received but not yet consumed.
    pub fn unconsumed(&self) -> usize {
        self.shared.stream.unconsumed()
    }

    /// Whether the wire is known dead or the stream ended.
    pub fn is_closed(&self) -> bool {
        self.shared.stream.ended() || self.shared.wire.pending.is_dead()
    }

    /// Human input — DATA chunks of at most [`INPUT_CHUNK`] bytes. Empty
    /// input sends nothing. An observer is refused with
    /// [`AttachError::Observe`], an ended stream with [`AttachError::Closed`].
    pub async fn send_input(&self, bytes: &[u8]) -> Result<(), AttachError> {
        self.live()?;
        self.send_stream(Kind::Data, bytes).await
    }

    /// An automatic reply to the program's query — AUTOMATIC_INPUT only
    /// when both `hello` and the attach reply confirmed it, otherwise the
    /// legacy DATA fallback. Facts never masquerade as human input.
    pub async fn reply(&self, bytes: &[u8]) -> Result<(), AttachError> {
        self.live()?;
        let kind = if self.shared.automatic_input {
            Kind::AutomaticInput
        } else {
            Kind::Data
        };
        self.send_stream(kind, bytes).await
    }

    /// Chunks `bytes` into frames and queues them, waiting on bounded
    /// capacity. A cancelled send leaves queued whole frames to the writer.
    async fn send_stream(&self, kind: Kind, bytes: &[u8]) -> Result<(), AttachError> {
        if bytes.is_empty() {
            return Ok(());
        }
        for chunk in bytes.chunks(INPUT_CHUNK) {
            let frame = Frame::new(self.shared.channel, kind, chunk.to_vec());
            self.shared
                .data
                .push(frame)
                .await
                .map_err(|()| AttachError::Closed)?;
        }
        Ok(())
    }

    /// Active-viewer grid change. Never sent by an observer or on a
    /// closed stream.
    pub async fn resize(&self, cols: u16, rows: u16) -> Result<(), AttachError> {
        self.live()?;
        self.control(Frame::resize(self.shared.channel, cols, rows))
            .await
    }

    /// Reports focus to the core. Never sent by an observer or on a
    /// closed stream.
    pub async fn focus(&self, focused: bool) -> Result<(), AttachError> {
        self.live()?;
        self.control(Frame::focus(self.shared.channel, focused))
            .await
    }

    /// Reports the viewer's own display facts — never as human input.
    /// Rejects a serialization over [`crate::frame::MAX_PAYLOAD`] before
    /// encoding, so `Frame::encode` can never panic here.
    pub async fn facts(&self, display: &Display) -> Result<(), AttachError> {
        self.live()?;
        let payload = serde_json::to_vec(display).map_err(CallError::Shape)?;
        self.control(Frame::facts(self.shared.channel, payload))
            .await
    }

    /// A control frame onto the priority queue; the wire writes it whole.
    /// Payloads over `MAX_PAYLOAD` are refused before they ever encode.
    /// A send parked on a full queue still observes a latched stream end
    /// — an inbound CLOSE releases it instead of leaving it parked.
    async fn control(&self, frame: Frame) -> Result<(), AttachError> {
        if frame.payload.len() > crate::frame::MAX_PAYLOAD {
            return Err(CallError::TooLarge {
                size: frame.payload.len(),
            }
            .into());
        }
        tokio::select! {
            sent = self.shared.wire.tx.send(frame) => {
                sent.map_err(|_| AttachError::Closed)
            }
            _ = self.shared.stream.wait_ended() => Err(AttachError::Closed),
        }
    }

    /// Refuses observer writes and any write on an ended stream.
    fn live(&self) -> Result<(), AttachError> {
        if self.shared.observe {
            return Err(AttachError::Observe);
        }
        if self.shared.stream.ended() {
            return Err(AttachError::Closed);
        }
        Ok(())
    }

    /// Acknowledges `n` consumed DATA bytes as credit to the core. Only
    /// real DATA bytes count — never frame headers or control payloads.
    /// Synchronous, so nothing can cancel it mid-bookkeeping.
    pub fn consume(&self, n: usize) -> Result<(), AttachError> {
        self.shared
            .stream
            .consume(n)
            .map_err(|(asked, outstanding)| AttachError::OverCredit { asked, outstanding })
    }

    /// The next stream event, or `None` once the stream ended and its
    /// backlog drained.
    pub async fn recv(&self) -> Option<Output> {
        self.shared.stream.recv().await
    }

    /// Graceful end: sends CLOSE and waits for the core's CLOSE ack with a
    /// deadline, then shuts the connection down. A stream that already
    /// ended reports its stored reason without resending. The terminal is
    /// never killed.
    pub async fn detach(self) -> Result<Ended, AttachError> {
        let shared = Arc::clone(&self.shared);
        let reason = if shared.stream.ended() {
            shared.stream.close_reason()
        } else {
            // One deadline covers the CLOSE enqueue *and* the ack wait —
            // a full control queue behind a non-reading peer can never
            // park detach past its budget.
            let send = shared
                .wire
                .tx
                .send(Frame::close(shared.channel, Vec::new()));
            match tokio::time::timeout(DETACH_TIMEOUT, async {
                let _ = send.await;
                shared.stream.wait_ended().await;
            })
            .await
            {
                Ok(()) => shared.stream.close_reason(),
                Err(_) => None,
            }
        };
        // Owned cleanup before the first await after this: pending fails,
        // the stream ends, tasks are aborted then awaited so both halves
        // drop, and the taken guard closes the backend. A cancelled
        // detach still kills and reaps the child.
        super::pump::fail(&shared.wire.pending, None);
        shared.stream.fail(None);
        shared.data.close();
        let guard = lock(&shared.guard).take();
        let tasks = lock(&shared.tasks).drain(..).collect::<Vec<_>>();
        for task in &tasks {
            task.abort();
        }
        for task in tasks {
            let _ = task.await;
        }
        let transport = match guard {
            Some(guard) => guard.close().await.map_err(with_transport_diag_detach)?,
            None => transport::Closed::default(),
        };
        Ok(Ended { reason, transport })
    }
}

/// `detach`'s backend-close failure surfaces as an open-class error; the
/// wire is already down either way.
fn with_transport_diag_detach(e: transport::Error) -> AttachError {
    AttachError::Open(super::error::Error::Transport(e))
}

impl Drop for Attach {
    /// Fail pending calls, end the stream, stop the tasks and drop the
    /// guard — which kills and reaps an ssh child. Synchronous, so
    /// waiters wake immediately.
    fn drop(&mut self) {
        super::pump::fail(&self.shared.wire.pending, None);
        self.shared.stream.fail(None);
        self.shared.data.close();
        for task in lock(&self.shared.tasks).drain(..) {
            task.abort();
        }
        drop(lock(&self.shared.guard).take());
    }
}

impl std::fmt::Debug for Attach {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Attach")
            .field("terminal", &self.shared.terminal.address)
            .field("channel", &self.shared.channel)
            .field("observe", &self.shared.observe)
            .field("closed", &self.is_closed())
            .finish_non_exhaustive()
    }
}

// Derived from herdr-core crates/herdr-core/src/server.rs and wire.rs (Apache-2.0).

//! The attach connection's outbound input queue. One [`DataQueue`] per
//! channel feeds the connection's single writer, which drains control
//! frames and owed credit first, then input fairly across channels —
//! so CLOSE/CREDIT stay responsive while input is backed up and one
//! backed-up channel cannot starve another's input or CLOSE. The
//! reader/writer halves themselves live in [`super::mux`].

use crate::client::lock;
use crate::frame::Frame;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{Notify, watch};

/// Queued-but-unwritten control frames (CLOSE, RESIZE, FOCUS, FACTS,
/// channel-0 calls). Full applies backpressure to senders; also the
/// per-round drain budget so a flooding producer cannot starve input.
pub(super) const CTRL_QUEUE: usize = 64;
/// Queued input frames (DATA/AUTOMATIC_INPUT chunks).
const DATA_QUEUE_FRAMES: usize = 64;
/// Queued input bytes — symmetric with the receive credit window.
const DATA_QUEUE_BYTES: usize = crate::frame::INITIAL_CREDIT as usize;

/// One channel's outbound stream input, bounded by frames *and* bytes.
/// Producers await capacity on a broadcast wake — every pop lets every
/// waiter recheck, so a producer needing a big chunk cannot starve
/// smaller ones behind it. A cancelled `send` simply stops waiting —
/// already-queued whole frames are the writer's, never half-sent.
pub(super) struct DataQueue {
    inner: Mutex<Queue>,
    /// Wakes the connection's writer — shared across channels, so a push
    /// on any channel wakes the one writer that serves all of them.
    wake: Arc<Notify>,
    /// Capacity for producers — broadcast: every pop wakes all waiters.
    room: watch::Sender<()>,
    /// Frames ever accepted — the write fence's target count.
    enqueued: AtomicU64,
    /// Frames the wire writer actually completed — broadcast per write
    /// so `wait_drained` waiters recheck on progress.
    written: watch::Sender<u64>,
    /// The wire writer is gone — a drain can never complete.
    dead: AtomicBool,
}

struct Queue {
    frames: VecDeque<Frame>,
    bytes: usize,
    closed: bool,
}

impl DataQueue {
    /// `work` is the connection writer's shared wakeup.
    pub(super) fn new(work: Arc<Notify>) -> DataQueue {
        let (room, _) = watch::channel(());
        let (written, _) = watch::channel(0u64);
        DataQueue {
            inner: Mutex::new(Queue {
                frames: VecDeque::new(),
                bytes: 0,
                closed: false,
            }),
            wake: work,
            room,
            enqueued: AtomicU64::new(0),
            written,
            dead: AtomicBool::new(false),
        }
    }

    /// Queues one complete frame, waiting for byte/frame capacity.
    /// `Err(())` when the queue is closed.
    pub(super) async fn push(&self, frame: Frame) -> Result<(), ()> {
        let len = frame.payload.len();
        let mut room = self.room.subscribe();
        loop {
            {
                let mut q = lock(&self.inner);
                if q.closed {
                    return Err(());
                }
                if q.frames.len() < DATA_QUEUE_FRAMES && q.bytes + len <= DATA_QUEUE_BYTES {
                    q.frames.push_back(frame);
                    q.bytes += len;
                    // Published under the same lock as the frame itself —
                    // a fence snapshot between enqueue and count could
                    // otherwise resolve early and let close overtake it.
                    self.enqueued.fetch_add(1, Ordering::AcqRel);
                    drop(q);
                    self.wake.notify_one();
                    return Ok(());
                }
            }
            if room.changed().await.is_err() {
                return Err(());
            }
        }
    }

    /// The next frame for the writer, releasing its byte capacity.
    pub(super) fn pop(&self) -> Option<Frame> {
        let frame = {
            let mut q = lock(&self.inner);
            let frame = q.frames.pop_front()?;
            q.bytes -= frame.payload.len();
            frame
        };
        let _ = self.room.send(());
        Some(frame)
    }

    /// Whether producers can still queue.
    pub(super) fn closed(&self) -> bool {
        lock(&self.inner).closed
    }

    /// Ends the queue: producers get `Err`, the writer drains what's left
    /// then sees the channel's slot dead. Every channel teardown path
    /// (stream end, detach, Drop, connection death) funnels here — a
    /// drain fence waiting on a socket-blocked writer must wake as
    /// failed now, not hang until the writer's own exit.
    pub(super) fn close(&self) {
        lock(&self.inner).closed = true;
        self.dead.store(true, Ordering::Release);
        self.written.send_modify(|_| {});
        self.wake.notify_one();
        let _ = self.room.send(());
    }

    /// One queued frame made it onto the wire — the drain fence counts
    /// actual transport writes, not queue positions.
    pub(super) fn mark_written(&self) {
        self.written.send_modify(|n| *n += 1);
    }

    /// The writer exited (dead wire or closed queues) — waiters that
    /// can never complete wake and fail instead of parking forever.
    pub(super) fn mark_dead(&self) {
        self.dead.store(true, Ordering::Release);
        self.written.send_modify(|_| {});
    }

    /// Write-completion fence: resolves once every frame queued so far
    /// has been written to the transport — the ordered local boundary
    /// before a graceful close, since CLOSE rides the control queue
    /// and would otherwise overtake queued input. `Err` once the
    /// writer is gone, so a caller still closes instead of waiting on
    /// undeliverable input.
    pub(super) async fn wait_drained(&self) -> Result<(), ()> {
        let target = self.enqueued.load(Ordering::Acquire);
        let mut rx = self.written.subscribe();
        loop {
            if *rx.borrow() >= target {
                return Ok(());
            }
            if self.dead.load(Ordering::Acquire) {
                return Err(());
            }
            rx.changed().await.map_err(|_| ())?;
        }
    }
}

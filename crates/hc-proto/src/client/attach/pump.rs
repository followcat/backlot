// Derived from herdr-core crates/herdr-core/src/server.rs and wire.rs (Apache-2.0).

//! The attach connection's two background halves. The reader demultiplexes:
//! channel 0 keeps JSON-RPC replies for the `terminal.attach` call itself,
//! stream channels go to the ordered bounded backlog. The writer owns every
//! complete frame write — control frames and owed credit always go ahead of
//! stream input, so CLOSE/CREDIT stay responsive while input is backed up.

use super::stream::Stream;
use crate::client::lock;
use crate::client::pump::{self, Pending};
use crate::codec::{Reader, Writer};
use crate::frame::{CONTROL_CHANNEL, Frame};
use crate::transport::{ReadHalf, WriteHalf};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{Notify, mpsc, watch};

/// Queued-but-unwritten control frames (CLOSE, RESIZE, FOCUS, FACTS,
/// channel-0 calls). Full applies backpressure to senders; also the
/// per-round drain budget so a flooding producer cannot starve input.
pub(super) const CTRL_QUEUE: usize = 64;
/// Queued input frames (DATA/AUTOMATIC_INPUT chunks).
const DATA_QUEUE_FRAMES: usize = 64;
/// Queued input bytes — symmetric with the receive credit window.
const DATA_QUEUE_BYTES: usize = crate::frame::INITIAL_CREDIT as usize;

/// Outbound stream input, bounded by frames *and* bytes. Producers await
/// capacity on a broadcast wake — every pop lets every waiter recheck, so
/// a producer needing a big chunk cannot starve smaller ones behind it.
/// A cancelled `send` simply stops waiting — already-queued whole frames
/// are the writer's, never half-sent.
pub(super) struct DataQueue {
    inner: Mutex<Queue>,
    /// Work for the writer.
    wake: Notify,
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
    pub(super) fn new() -> DataQueue {
        let (room, _) = watch::channel(());
        let (written, _) = watch::channel(0u64);
        DataQueue {
            inner: Mutex::new(Queue {
                frames: VecDeque::new(),
                bytes: 0,
                closed: false,
            }),
            wake: Notify::new(),
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
    fn pop(&self) -> Option<Frame> {
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
    fn closed(&self) -> bool {
        lock(&self.inner).closed
    }

    /// Ends the queue: producers get `Err`, the writer drains what's left
    /// then exits once the control queue also ended. Every shutdown path
    /// (reader EOF, stream end, detach, Drop) funnels here — a drain
    /// fence waiting on a socket-blocked writer must wake as failed now,
    /// not hang until the writer's own exit.
    pub(super) fn close(&self) {
        lock(&self.inner).closed = true;
        self.dead.store(true, Ordering::Release);
        self.written.send_modify(|_| {});
        self.wake.notify_one();
        let _ = self.room.send(());
    }

    /// Work notify registration for the writer's park.
    fn wakes(&self) -> &Notify {
        &self.wake
    }

    /// One queued frame made it onto the wire — the drain fence counts
    /// actual transport writes, not queue positions.
    fn mark_written(&self) {
        self.written.send_modify(|n| *n += 1);
    }

    /// The writer exited (dead wire or closed queues) — waiters that
    /// can never complete wake and fail instead of parking forever.
    fn mark_dead(&self) {
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

/// Reads frames until the stream ends or the peer violates the protocol.
/// Channel-0 replies feed the pending table; everything else goes to the
/// attach backlog — the reader never waits on the consumer, so a stalled
/// `recv` can never hide a CLOSE. An inbound CLOSE ends the connection's
/// input queue too, releasing queued/parked producers. Ends by failing
/// pending calls and the stream, then runs `on_end` (full shutdown).
pub(super) async fn read_loop(
    mut reader: Reader<ReadHalf>,
    pending: Pending,
    stream: Arc<Stream>,
    data: Arc<DataQueue>,
    on_end: impl FnOnce(),
) {
    let mut reason = None;
    while let Ok(Some(frame)) = reader.read_frame().await {
        let bad = if frame.channel == CONTROL_CHANNEL {
            pump::dispatch(frame, &pending).err()
        } else {
            stream.push(frame).err()
        };
        if stream.ended() {
            data.close();
        }
        if let Some(why) = bad {
            reason = Some(why);
            break;
        }
    }
    pump::fail(&pending, reason.clone());
    stream.fail(reason);
    data.close();
    on_end();
}

/// Writes queued frames until the queues close or the wire fails. Each
/// round drains a bounded batch of control frames and owed credit before
/// a bounded batch of stream input — CLOSE/CREDIT always go first, but a
/// flooding producer can starve neither direction. Frames are written
/// whole: a cancelled producer can never split one on the wire.
pub(super) async fn write_loop(
    mut writer: Writer<WriteHalf>,
    mut ctrl: mpsc::Receiver<Frame>,
    data: Arc<DataQueue>,
    stream: Arc<Stream>,
    on_end: impl FnOnce(),
) {
    let mut ctrl_open = true;
    'outer: loop {
        let mut worked = false;
        for _ in 0..CTRL_QUEUE {
            if !ctrl_open {
                break;
            }
            match ctrl.try_recv() {
                Ok(frame) => {
                    worked = true;
                    if writer.write_frame(&frame).await.is_err() {
                        break 'outer;
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    ctrl_open = false;
                    break;
                }
            }
        }
        if let Some(frame) = stream.take_owed() {
            worked = true;
            if writer.write_frame(&frame).await.is_err() {
                break;
            }
        }
        for _ in 0..16 {
            let Some(frame) = data.pop() else { break };
            worked = true;
            if writer.write_frame(&frame).await.is_err() {
                break 'outer;
            }
            data.mark_written();
        }
        if worked {
            continue;
        }
        if !ctrl_open && data.closed() && stream.owed() == 0 {
            break; // producers all gone and drained: the wire is done
        }
        tokio::select! {
            frame = ctrl.recv(), if ctrl_open => {
                match frame {
                    Some(frame) => {
                        if writer.write_frame(&frame).await.is_err() { break 'outer; }
                    }
                    None => ctrl_open = false,
                }
            }
            _ = data.wakes().notified() => {}
            _ = stream.wakes().notified() => {}
        }
    }
    // Dropping the writer half-closes our side: the peer sees stdin EOF.
    data.mark_dead();
    drop(writer);
    on_end();
}

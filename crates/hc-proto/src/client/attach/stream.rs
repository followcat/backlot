// Derived from herdr-core crates/herdr-core/src/viewers.rs and wire.rs (Apache-2.0).

//! The attach stream's receive side: an ordered, bounded event backlog the
//! reader task fills without ever parking on the consumer — so a stalled
//! consumer can never hide a CLOSE — plus the credit bookkeeping that only
//! acknowledges bytes the caller actually consumed.
//!
//! Wakeups broadcast: `recv`/`wait_ended` waiters each hold a `watch`
//! receiver, so a CLOSE or failure reaches every waiter, not just one.

use super::output::{CloseReason, Output};
use crate::client::lock;
use crate::frame::{Control, Frame, INITIAL_CREDIT, Kind};
use std::collections::VecDeque;
use std::sync::Mutex;
use tokio::sync::{Notify, watch};

/// Consecutive DATA payloads coalesce into one event up to this size —
/// bounded, in order, so `recv` chunks stay predictable.
const COALESCE_MAX: usize = 64 * 1024;
/// Payload bytes queued for the consumer: the credit window plus in-flight
/// and control slack. An honest core cannot outrun it.
const MAX_BACKLOG_BYTES: usize = INITIAL_CREDIT as usize + 64 * 1024;
/// Events (frames) queued for the consumer — every event kind counts,
/// including RESIZE and CLOSE.
const MAX_BACKLOG_EVENTS: usize = 1024;

/// What `pop` found for a `recv` call.
enum Pop {
    Event(Output),
    /// Nothing yet: wait on the broadcast.
    Wait,
    /// Ended and drained.
    Done,
}

/// Shared stream state: routing, the bounded event backlog and credit
/// bookkeeping under one lock. The reader task holds `Arc<Stream>` — never
/// the `Attach`'s shared state — so handle drop is prompt.
pub(super) struct Stream {
    inner: Mutex<Inner>,
    /// Broadcast to every `recv`/`wait_ended` waiter.
    events: watch::Sender<()>,
    /// Wakes the one writer task to flush owed credit.
    credit_wake: Notify,
}

/// A queued event plus the backlog bytes it was charged with — `pop`
/// releases exactly that, so zero-payload `Output` variants never leak
/// accounting (a RESIZE frame still carried 4 wire bytes).
struct Event {
    output: Output,
    charged: usize,
}

struct Inner {
    /// The negotiated channel; `None` until `terminal.attach` replies.
    channel: Option<u32>,
    /// Non-control frames seen before install — replayed in order then.
    held: VecDeque<Frame>,
    /// Ordered events for the consumer.
    queue: VecDeque<Event>,
    /// Payload bytes held+queued (backlog bound).
    bytes: usize,
    /// DATA bytes received minus consumed: what credit may acknowledge.
    unconsumed: usize,
    /// Consumed bytes not yet written as CREDIT — the writer flushes them.
    owed: u64,
    /// How the stream ended, once.
    end: Option<End>,
    /// The terminal `Failed` event was already synthesized.
    reported: bool,
}

enum End {
    Closed(CloseReason),
    Failed(String),
}

impl Stream {
    pub(super) fn new() -> Stream {
        let (events, _) = watch::channel(());
        Stream {
            inner: Mutex::new(Inner {
                channel: None,
                held: VecDeque::new(),
                queue: VecDeque::new(),
                bytes: 0,
                unconsumed: 0,
                owed: 0,
                end: None,
                reported: false,
            }),
            events,
            credit_wake: Notify::new(),
        }
    }

    /// Wakes every `recv`/`wait_ended` waiter (a new event or the end).
    fn wake(&self) {
        let _ = self.events.send(());
    }

    /// Installs the negotiated channel and replays held frames atomically
    /// under the state lock — a concurrently arriving frame can never
    /// jump ahead of older held ones. Replay uses the same channel policy
    /// as `push`: ours become events, foreign ones `Output::Control`.
    /// Held frames were already charged, so their charge moves, not adds.
    pub(super) fn install(&self, channel: u32) -> Result<(), String> {
        let mut i = lock(&self.inner);
        i.channel = Some(channel);
        let held: VecDeque<Frame> = std::mem::take(&mut i.held);
        for frame in held {
            i.bytes -= frame.payload.len();
            if frame.channel == channel {
                self.event(frame, &mut i)?;
            } else {
                self.charge(&mut i, frame.payload.len())?;
                let (charged, output) = control_of(frame);
                self.queue_event(&mut i, output, charged)?;
            }
        }
        drop(i);
        self.wake();
        Ok(())
    }

    /// One inbound frame for this stream: held while the channel is
    /// unknown; once installed, ours become events and foreign-channel
    /// frames surface as `Output::Control`. `Err` is a protocol
    /// violation — the reader ends the wire.
    pub(super) fn push(&self, frame: Frame) -> Result<(), String> {
        let mut i = lock(&self.inner);
        if i.end.is_some() {
            return Ok(()); // already over: drop trailing frames
        }
        match i.channel {
            None => {
                i.bytes += frame.payload.len();
                if i.bytes > MAX_BACKLOG_BYTES || i.held.len() >= MAX_BACKLOG_EVENTS {
                    return Err("backlog bound exceeded before channel install".into());
                }
                i.held.push_back(frame);
                Ok(())
            }
            Some(ch) if frame.channel == ch => self.event(frame, &mut i),
            Some(_) => {
                self.charge(&mut i, frame.payload.len())?;
                let (charged, output) = control_of(frame);
                self.queue_event(&mut i, output, charged)?;
                drop(i);
                self.wake();
                Ok(())
            }
        }
    }

    /// Converts a routed frame into events; `Err` is a protocol violation.
    /// Every frame is byte-charged; every *new* event is count-charged —
    /// a RESIZE flood cannot grow the backlog unboundedly. Each queued
    /// event keeps its charge so `pop` releases exactly these bytes.
    fn event(&self, frame: Frame, i: &mut Inner) -> Result<(), String> {
        self.charge(i, frame.payload.len())?;
        let charged = frame.payload.len();
        match frame.kind {
            Kind::Data => {
                i.unconsumed += charged;
                match i.queue.back_mut() {
                    Some(Event {
                        output: Output::Data(bytes),
                        charged: held,
                    }) if bytes.len() + charged <= COALESCE_MAX => {
                        bytes.extend_from_slice(&frame.payload);
                        *held += charged;
                    }
                    _ => self.queue_event(i, Output::Data(frame.payload), charged)?,
                }
            }
            Kind::Resize => {
                let Ok(Control::Resize { cols, rows }) = frame.control() else {
                    return Err("malformed RESIZE frame".into());
                };
                self.queue_event(i, Output::Resize { cols, rows }, charged)?;
            }
            Kind::Close => {
                let reason = CloseReason::parse(&frame.payload);
                self.queue_event(i, Output::Closed(reason.clone()), charged)?;
                i.end = Some(End::Closed(reason));
            }
            _ => {
                let (charged, output) = control_of(frame);
                self.queue_event(i, output, charged)?;
            }
        }
        self.wake();
        Ok(())
    }

    /// Queues one byte-charged event, bounded by the event count.
    fn queue_event(&self, i: &mut Inner, output: Output, charged: usize) -> Result<(), String> {
        self.charge_event(i)?;
        i.queue.push_back(Event { output, charged });
        Ok(())
    }

    /// Charges backlog bytes; excess is a protocol violation.
    fn charge(&self, i: &mut Inner, bytes: usize) -> Result<(), String> {
        i.bytes += bytes;
        if i.bytes > MAX_BACKLOG_BYTES {
            return Err("the core outran the credit window".into());
        }
        Ok(())
    }

    /// Charges one backlog event; a flood of zero-byte controls is bounded.
    fn charge_event(&self, i: &mut Inner) -> Result<(), String> {
        if i.queue.len() >= MAX_BACKLOG_EVENTS {
            return Err("event flood on the attach stream".into());
        }
        Ok(())
    }

    /// The next event for `recv`.
    fn pop(&self) -> Pop {
        let mut i = lock(&self.inner);
        if let Some(event) = i.queue.pop_front() {
            i.bytes = i.bytes.saturating_sub(event.charged);
            return Pop::Event(event.output);
        }
        let unreported_failure = match &i.end {
            Some(End::Failed(reason)) if !i.reported => Some(reason.clone()),
            _ => None,
        };
        match (unreported_failure, i.end.is_some()) {
            (Some(reason), _) => {
                i.reported = true;
                Pop::Event(Output::Failed(reason))
            }
            (None, true) => Pop::Done,
            (None, false) => Pop::Wait,
        }
    }

    /// Whether the stream ended (CLOSE, failure or wire EOF).
    pub(super) fn ended(&self) -> bool {
        lock(&self.inner).end.is_some()
    }

    /// The stored CLOSE reason, when the core sent one.
    pub(super) fn close_reason(&self) -> Option<CloseReason> {
        match &lock(&self.inner).end {
            Some(End::Closed(reason)) => Some(reason.clone()),
            _ => None,
        }
    }

    /// Waits until the stream ends (the detach ack is a CLOSE too).
    /// Broadcast wakeup: a shared `watch` flag means every waiter sees it.
    pub(super) async fn wait_ended(&self) {
        let mut wake = self.events.subscribe();
        loop {
            if self.ended() {
                return;
            }
            let _ = wake.changed().await;
        }
    }

    /// Ends the stream from our side: `reason` becomes the terminal
    /// `Failed` event; `None` reports a plain end. Wakees drain then see it.
    pub(super) fn fail(&self, reason: Option<String>) {
        {
            let mut i = lock(&self.inner);
            if i.end.is_none() {
                i.end = Some(match reason {
                    Some(why) => End::Failed(why),
                    None => End::Failed("the wire ended".into()),
                });
            }
        }
        self.wake();
    }

    /// A `recv` event; blocks until one exists or the stream ended+drained.
    /// Broadcast wakeup: every waiter sees each change — no receiver can be
    /// left parked by a wake spent on a sibling.
    pub(super) async fn recv(&self) -> Option<Output> {
        let mut wake = self.events.subscribe();
        loop {
            match self.pop() {
                Pop::Event(o) => return Some(o),
                Pop::Done => return None,
                Pop::Wait => {
                    if wake.changed().await.is_err() {
                        return match self.pop() {
                            Pop::Event(o) => Some(o),
                            _ => None,
                        };
                    }
                }
            }
        }
    }

    /// Acknowledges `n` consumed DATA bytes — never received, header or
    /// control bytes. Synchronous: a caller can never be cancelled between
    /// the bookkeeping and the credit, so nothing is lost or duplicated.
    pub(super) fn consume(&self, n: usize) -> Result<(), (usize, usize)> {
        if n == 0 {
            return Ok(());
        }
        let mut i = lock(&self.inner);
        if n > i.unconsumed {
            return Err((n, i.unconsumed));
        }
        i.unconsumed -= n;
        i.owed += n as u64;
        drop(i);
        self.credit_wake.notify_one();
        Ok(())
    }

    /// DATA bytes received but not yet consumed.
    pub(super) fn unconsumed(&self) -> usize {
        lock(&self.inner).unconsumed
    }

    /// Bytes waiting to be written as CREDIT.
    pub(super) fn owed(&self) -> u64 {
        lock(&self.inner).owed
    }

    /// Takes owed credit as a CREDIT frame for the writer — `None` when
    /// nothing is owed or the channel is not installed yet. The debt is
    /// only cleared once the frame exists, and a single frame never
    /// acknowledges more than `u32::MAX` bytes; the rest stays owed.
    pub(super) fn take_owed(&self) -> Option<Frame> {
        let mut i = lock(&self.inner);
        let ch = i.channel?;
        if i.owed == 0 {
            return None;
        }
        let n = i.owed.min(u32::MAX as u64);
        i.owed -= n;
        Some(Frame::credit(ch, n as u32))
    }

    /// Notify registrations for the writer's park.
    pub(super) fn wakes(&self) -> &Notify {
        &self.credit_wake
    }
}

/// A foreign-channel or unknown-kind frame as a `Control` event plus the
/// backlog bytes it was charged with — the payload is preserved untouched.
fn control_of(frame: Frame) -> (usize, Output) {
    (
        frame.payload.len(),
        Output::Control {
            channel: frame.channel,
            kind: frame.kind.to_u8(),
            payload: frame.payload,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `pop` must release exactly the bytes `event`/`push` charged —
    /// RESIZE and zero-payload Control surfaces used to leak theirs.
    #[tokio::test]
    async fn drained_events_release_their_charged_bytes() {
        let stream = Stream::new();
        stream.install(1).unwrap();
        for frame in [
            Frame::resize(1, 120, 40),
            Frame::data(1, b"abc".to_vec()),
            Frame::new(9, Kind::Data, b"foreign".to_vec()),
            Frame::close(1, b"{\"reason\":\"exited\"}".to_vec()),
        ] {
            stream.push(frame).unwrap();
        }
        assert_eq!(
            stream.recv().await,
            Some(Output::Resize {
                cols: 120,
                rows: 40
            })
        );
        assert_eq!(stream.recv().await, Some(Output::Data(b"abc".to_vec())));
        assert_eq!(
            stream.recv().await,
            Some(Output::Control {
                channel: 9,
                kind: 0,
                payload: b"foreign".to_vec(),
            })
        );
        assert!(matches!(stream.recv().await, Some(Output::Closed(_))));
        assert_eq!(stream.recv().await, None);
        assert_eq!(lock(&stream.inner).bytes, 0, "charged bytes leaked");
    }
}

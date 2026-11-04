// Derived from herdr-core crates/herdr-core/src/server.rs (Apache-2.0).

//! The background halves of a framed client: a reader task demultiplexing
//! channel-0 replies by id, and a writer task owning every complete frame
//! write so a cancelled caller can never split a frame on the wire.

use super::error::CallError;
use crate::codec::{Reader, Writer};
use crate::frame::{CONTROL_CHANNEL, Frame, Kind};
use crate::jsonrpc::{Id, Message, Response};
use crate::transport::{ReadHalf, WriteHalf};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc::Receiver, oneshot};

type Senders = HashMap<Id, oneshot::Sender<Result<Value, CallError>>>;

/// Replies in flight: one oneshot per unanswered call id. The dead flag is
/// stored under the same lock as the map so registration and the final
/// fail-all are atomic — a call can never register after the sweep.
#[derive(Default)]
struct PendingInner {
    map: Mutex<Senders>,
    /// Wire-dead: set inside `fail` while the map is locked.
    dead: AtomicBool,
}

/// Shared handle to the pending table (`Arc` keeps it cheap for every
/// task/guard to hold).
#[derive(Default, Clone)]
pub(super) struct Pending(Arc<PendingInner>);

impl Pending {
    /// Locks the pending map, recovering from a poisoned lock like
    /// `transport` does for its tail: a panicking task must not wedge the
    /// survivors.
    fn map(&self) -> std::sync::MutexGuard<'_, Senders> {
        self.0.map.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Whether the wire is known dead.
    pub(super) fn is_dead(&self) -> bool {
        self.0.dead.load(Ordering::SeqCst)
    }

    /// Calls currently waiting for a reply.
    pub(super) fn len(&self) -> usize {
        self.map().len()
    }
}

/// Reads frames until the stream ends, fails, or the core violates the
/// protocol, then fails every pending call (`Protocol` on a violation,
/// `Closed` otherwise) and runs `on_end` — which initiates full shutdown.
/// Notifications and non-zero channels are parsed but unused until the
/// subscription/attach layers exist; replies to unknown or cancelled ids
/// are dropped, never fatal.
pub(super) async fn read_loop(
    mut reader: Reader<ReadHalf>,
    pending: Pending,
    on_end: impl FnOnce(),
) {
    let mut reason = None;
    while let Ok(Some(frame)) = reader.read_frame().await {
        if let Err(why) = dispatch(frame, &pending) {
            reason = Some(why);
            break;
        }
    }
    fail(&pending, reason);
    on_end();
}

/// Writes queued frames until the queue closes or the stream fails. Each
/// queued item is a whole frame, so whichever task owns this can only ever
/// write complete frames. `on_end` initiates the same unified shutdown the
/// reader runs, so a write failure cannot leave callers waiting.
pub(super) async fn write_loop(
    mut writer: Writer<WriteHalf>,
    mut rx: Receiver<Frame>,
    on_end: impl FnOnce(),
) {
    while let Some(frame) = rx.recv().await {
        if writer.write_frame(&frame).await.is_err() {
            break;
        }
    }
    on_end();
}

/// One frame from the core: channel-0 DATA replies go to their caller.
/// `Err` means a protocol violation — a channel-0 payload that is not a
/// valid JSON-RPC message, or a response with a null id; both are wire
/// contract failures, not per-call shape errors. Valid notifications and
/// requests have no consumer yet and are ignored; replies to unknown or
/// cancelled ids are dropped.
pub(super) fn dispatch(frame: Frame, pending: &Pending) -> Result<(), String> {
    if frame.channel != CONTROL_CHANNEL || frame.kind != Kind::Data {
        return Ok(());
    }
    let message = Message::parse(&frame.payload)
        .map_err(|e| format!("malformed message on the control channel: {e}"))?;
    let Message::Response(response) = message else {
        return Ok(());
    };
    let Some(id) = response.id().cloned() else {
        return Err("response with a null id on the control channel".into());
    };
    let sender = pending.map().remove(&id);
    let Some(sender) = sender else {
        return Ok(()); // late reply to a timed-out or cancelled call: safe to drop
    };
    let result = match response {
        Response::Ok(r) => Ok(r.result),
        Response::Err(r) => Err(CallError::Rpc(r.error)),
    };
    let _ = sender.send(result);
    // A caller may have been cancelled between frames: its dead oneshot
    // receiver frees no map slot until swept — do it while we are here.
    sweep(pending);
    Ok(())
}

/// Ends every pending call and marks the wire dead — under the map lock,
/// so no call can register afterwards and hang. `reason` makes the failure
/// a `Protocol` error; `None` reports plain `Closed`.
pub(super) fn fail(pending: &Pending, reason: Option<String>) {
    let senders = {
        let mut map = pending.map();
        pending.0.dead.store(true, Ordering::SeqCst);
        std::mem::take(&mut *map)
    };
    for (_, sender) in senders {
        let error = match &reason {
            Some(why) => CallError::Protocol(why.clone()),
            None => CallError::Closed,
        };
        let _ = sender.send(Err(error));
    }
}

/// Removes entries whose caller was cancelled (receiver dropped).
pub(super) fn sweep(pending: &Pending) {
    pending.map().retain(|_, sender| !sender.is_closed());
}

/// Registers `id`: `Closed` when the wire is dead (checked under the map
/// lock, atomic with `fail`), `Full` at the in-flight bound.
pub(super) fn register(
    pending: &Pending,
    id: Id,
    bound: usize,
) -> Result<oneshot::Receiver<Result<Value, CallError>>, CallError> {
    let mut map = pending.map();
    if pending.is_dead() {
        return Err(CallError::Closed);
    }
    if map.len() >= bound {
        return Err(CallError::Full);
    }
    let (tx, rx) = oneshot::channel();
    map.insert(id, tx);
    Ok(rx)
}

/// Removes a pending entry — timeout, a failed enqueue, or a cancelled
/// caller via [`Entry`].
pub(super) fn remove(pending: &Pending, id: &Id) {
    pending.map().remove(id);
}

/// Owns one pending registration until it drops: if the caller's future is
/// cancelled — whether it was blocked on the queue or awaiting a reply —
/// its map entry is removed eagerly instead of lingering for a later sweep.
pub(super) struct Entry {
    pending: Pending,
    id: Id,
}

impl Entry {
    pub(super) fn new(pending: Pending, id: Id) -> Entry {
        Entry { pending, id }
    }
}

impl Drop for Entry {
    fn drop(&mut self) {
        remove(&self.pending, &self.id);
    }
}

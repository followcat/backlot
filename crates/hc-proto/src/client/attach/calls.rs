// Derived from herdr-core crates/herdr-core/src/server.rs and wire.rs (Apache-2.0).

//! The `terminal.attach` call table and the call lifecycle around it.
//!
//! Unlike a pending RPC entry, an attach registration cannot simply
//! drop when its caller goes away: a request that already reached the
//! wire may still get a reply naming a channel the core allocated, and
//! that channel must still be CLOSED or the viewer leaks. So a call
//! cancelled before its send completes frees its id at once, while a
//! sent request whose caller timed out or was cancelled moves to a
//! bounded orphan set — late replies still close their channel. An
//! orphan set that fills means the core stopped answering attach calls
//! entirely: the connection is torn down so the next open rebuilds it
//! instead of every attach failing `Full` until the client restarts.

use super::conn::Conn;
use crate::client::call::MAX_INFLIGHT;
use crate::client::error::CallError;
use crate::client::lock;
use crate::frame::{CONTROL_CHANNEL, Frame, MAX_PAYLOAD};
use crate::jsonrpc::{Id, Request};
use crate::method;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::oneshot;

/// Replies still expected for attach requests whose caller went away.
/// Bounded like the in-flight table itself: a full orphan set means the
/// core stopped answering attach calls at all.
const MAX_ORPHANS: usize = MAX_INFLIGHT;

/// An attach call's deadline — the core answering HELLO but never
/// replying to `terminal.attach` must not hang open forever.
const ATTACH_TIMEOUT: Duration = Duration::from_secs(30);

/// `terminal.attach` calls whose reply may still land. A caller that
/// goes away before its send completes frees the id outright; after the
/// send the id becomes an orphan — the late reply still names a channel
/// that gets CLOSED, so the viewer is never leaked.
#[derive(Default)]
pub(super) struct AttachCalls {
    inner: Mutex<AttachCallsInner>,
}

#[derive(Default)]
struct AttachCallsInner {
    map: HashMap<Id, oneshot::Sender<Result<Value, CallError>>>,
    /// Sent requests whose caller went away — the reply still names a
    /// channel owed a CLOSE. Bounded by [`MAX_ORPHANS`].
    orphans: HashSet<Id>,
    /// Connection dead: set inside `fail_all` while the map is locked, so
    /// no call can register after the sweep and hang.
    dead: bool,
}

/// What `take` found for an inbound reply's id.
pub(super) enum Taken {
    /// A caller still waits on this reply.
    Waiting(oneshot::Sender<Result<Value, CallError>>),
    /// The request reached the wire but the caller went away — the
    /// reply still names a channel owed a CLOSE.
    Orphan,
    /// Not an attach call — a plain pending-table reply.
    Unknown,
}

impl AttachCalls {
    /// Registers `id`: `Closed` when the connection is dead (checked
    /// under the map lock, atomic with `fail_all`), `Full` at the
    /// in-flight bound.
    pub(super) fn register(
        &self,
        id: Id,
        bound: usize,
    ) -> Result<oneshot::Receiver<Result<Value, CallError>>, CallError> {
        let mut inner = lock(&self.inner);
        if inner.dead {
            return Err(CallError::Closed);
        }
        if inner.map.len() >= bound {
            return Err(CallError::Full);
        }
        let (tx, rx) = oneshot::channel();
        inner.map.insert(id, tx);
        Ok(rx)
    }

    /// Resolves an inbound reply's id.
    pub(super) fn take(&self, id: &Id) -> Taken {
        let mut inner = lock(&self.inner);
        if let Some(sender) = inner.map.remove(id) {
            return Taken::Waiting(sender);
        }
        if inner.orphans.remove(id) {
            return Taken::Orphan;
        }
        Taken::Unknown
    }

    /// The call ended without its reply: a registration cancelled before
    /// its request was sent frees the id at once; a sent request moves
    /// to the orphan set so a late reply still closes its channel. `true`
    /// when the orphan bound is already full — the core stopped
    /// answering attach calls, so the connection is treated as broken.
    pub(super) fn release(&self, id: &Id, sent: bool) -> bool {
        let mut inner = lock(&self.inner);
        if inner.map.remove(id).is_none() || !sent || inner.dead {
            return false;
        }
        if inner.orphans.len() >= MAX_ORPHANS {
            return true;
        }
        inner.orphans.insert(id.clone());
        false
    }

    /// Calls currently registered.
    pub(super) fn len(&self) -> usize {
        lock(&self.inner).map.len()
    }

    /// Fails every in-flight attach call and latches the table dead —
    /// under the map lock, so a later `register` sees `dead` and can
    /// never register into a dead connection.
    pub(super) fn fail_all(&self, reason: Option<String>) {
        let senders = {
            let mut inner = lock(&self.inner);
            inner.dead = true;
            inner.orphans.clear();
            std::mem::take(&mut inner.map)
        };
        for (_, sender) in senders {
            let error = match &reason {
                Some(why) => CallError::Protocol(why.clone()),
                None => CallError::Closed,
            };
            let _ = sender.send(Err(error));
        }
    }
}

impl Conn {
    /// The `terminal.attach` call itself: the reply rides `attach_calls`,
    /// and unlike a plain RPC entry the registration is not just dropped
    /// with its caller — a sent request may still get a reply naming a
    /// channel that must be CLOSED. `CallGuard` frees the id at once when
    /// the call is cancelled before the request reached the wire.
    pub(super) async fn attach_call(&self, params: Value) -> Result<Value, CallError> {
        let id = Id::Num(self.wire.next_id.fetch_add(1, Ordering::Relaxed));
        let request = Request::new(id.clone(), method::TERMINAL_ATTACH, Some(params));
        let payload = serde_json::to_vec(&request).map_err(CallError::Shape)?;
        if payload.len() > MAX_PAYLOAD {
            return Err(CallError::TooLarge {
                size: payload.len(),
            });
        }
        let rx = self.attach_calls.register(id.clone(), MAX_INFLIGHT)?;
        let mut guard = CallGuard::new(self, id);
        let frame = Frame::data(CONTROL_CHANNEL, payload);
        let result = match tokio::time::timeout(ATTACH_TIMEOUT, async {
            if self.wire.tx.send(frame).await.is_err() {
                return Err(CallError::Closed);
            }
            guard.sent = true;
            rx.await.unwrap_or(Err(CallError::Closed))
        })
        .await
        {
            Ok(result) => result,
            Err(_) => Err(CallError::Timeout),
        };
        if result.is_ok() {
            guard.disarm();
        }
        result
    }
}

/// Owns one attach-call registration for the call's life. A reply that
/// arrives in time removes the entry itself, so `disarm` follows it.
/// Every other exit — the call timed out, the send failed, or the
/// future was dropped parked on the full control queue or awaiting the
/// reply — releases the registration: an unsent id leaves at once, a
/// sent one becomes an orphan, and an orphan set past its bound tears
/// the connection down.
struct CallGuard<'a> {
    conn: &'a Conn,
    id: Id,
    sent: bool,
    done: bool,
}

impl<'a> CallGuard<'a> {
    fn new(conn: &'a Conn, id: Id) -> CallGuard<'a> {
        CallGuard {
            conn,
            id,
            sent: false,
            done: false,
        }
    }

    /// The reply arrived — `dispatch` already removed the registration.
    fn disarm(mut self) {
        self.done = true;
    }
}

impl Drop for CallGuard<'_> {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        if self.conn.attach_calls.release(&self.id, self.sent) {
            self.conn
                .died(Some("the core stopped answering attach calls".into()), true);
        }
    }
}

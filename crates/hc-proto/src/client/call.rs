// Derived from herdr-core schema/README.md and crates/herdr-core/src/server.rs (Apache-2.0).

//! The raw call path: one request's deadline covers both its queue
//! enqueue (a full queue behind a stalled peer) and its reply wait, and
//! dropping the future — whatever phase it is in — removes the pending
//! entry so a cancelled caller frees its slot immediately. Also the
//! optional `rpc.discover` round that refines method availability.

use super::error::CallError;
use super::{lock, pump};
use crate::OpenRpcDocument;
use crate::frame::{CONTROL_CHANNEL, Frame, MAX_PAYLOAD};
use crate::hello::{Capabilities, HelloResult};
use crate::jsonrpc::{Id, Request};
use crate::method;
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

/// `rpc.discover` is optional enrichment: it never gets the call deadline.
pub(super) const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);
/// Calls waiting for a reply at once.
pub(super) const MAX_INFLIGHT: usize = 256;

/// The parts of a connection a call needs. Background actors hold this —
/// never the whole `Shared` — so the last client handle's drop cleans up
/// promptly instead of waiting for a task to finish.
pub(super) struct Wire {
    /// Whole frames for the writer task.
    pub(super) tx: mpsc::Sender<Frame>,
    /// Pending calls; its dead flag is the wire's closed state.
    pub(super) pending: pump::Pending,
    pub(super) next_id: AtomicI64,
}

/// One raw framed call on the wire: the deadline covers both the enqueue
/// and the reply wait, and `Entry` removes the pending registration on
/// any exit — timeout, cancellation, or a reply that already won the
/// race (then the remove is a no-op).
pub(super) async fn wire_call(
    wire: &Wire,
    method: &str,
    params: Option<Value>,
    deadline: Duration,
) -> Result<Value, CallError> {
    let id = Id::Num(wire.next_id.fetch_add(1, Ordering::Relaxed));
    let request = Request::new(id.clone(), method, params);
    let payload = serde_json::to_vec(&request).map_err(CallError::Shape)?;
    if payload.len() > MAX_PAYLOAD {
        return Err(CallError::TooLarge {
            size: payload.len(),
        });
    }
    pump::sweep(&wire.pending);
    let rx = pump::register(&wire.pending, id.clone(), MAX_INFLIGHT)?;
    let _entry = pump::Entry::new(wire.pending.clone(), id);
    let frame = Frame::data(CONTROL_CHANNEL, payload);
    match tokio::time::timeout(deadline, async {
        if wire.tx.send(frame).await.is_err() {
            return Err(CallError::Closed);
        }
        match rx.await {
            Ok(result) => result,
            Err(_closed_sender) => Err(CallError::Closed),
        }
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(CallError::Timeout),
    }
}

/// The three discovery cells a [`super::Shared`] exposes to the task.
pub(super) type DiscoveryCells = (
    Arc<Mutex<Option<OpenRpcDocument>>>,
    Arc<Mutex<Option<BTreeSet<String>>>>,
    Arc<Mutex<Option<String>>>,
);

/// One `rpc.discover` round: bounded by [`DISCOVERY_TIMEOUT`], and a
/// missing method, unusable document, or failed call is a noted
/// limitation — never a crash, never a gate on `hello` capabilities.
pub(super) async fn discover(
    hello: &HelloResult,
    wire: &Wire,
    (document, methods, note): &DiscoveryCells,
) {
    if !Capabilities::new(hello)
        .map(|c| c.has_method(method::RPC_DISCOVER))
        .unwrap_or(false)
    {
        *lock(note) = Some(format!(
            "core {} does not offer `rpc.discover`",
            hello.core.name
        ));
        return;
    }
    match wire_call(wire, method::RPC_DISCOVER, None, DISCOVERY_TIMEOUT).await {
        Ok(doc) => match method_names(&doc) {
            Ok(names) => {
                *lock(methods) = Some(names);
                *lock(document) = Some(doc);
                *lock(note) = None;
            }
            Err(why) => *lock(note) = Some(why),
        },
        Err(e) => *lock(note) = Some(format!("rpc.discover failed: {e}")),
    }
}

/// Validates only the method-name list of an OpenRPC document — the rest
/// stays opaque. An unusable document is a diagnostic, not a crash.
fn method_names(doc: &OpenRpcDocument) -> Result<BTreeSet<String>, String> {
    let Some(methods) = doc.get("methods").and_then(Value::as_array) else {
        return Err("rpc.discover reply has no `methods` list".into());
    };
    let mut names = BTreeSet::new();
    for (i, entry) in methods.iter().enumerate() {
        let Some(name) = entry.get("name").and_then(Value::as_str) else {
            return Err(format!("rpc.discover methods[{i}] lacks a string `name`"));
        };
        names.insert(name.to_string());
    }
    Ok(names)
}

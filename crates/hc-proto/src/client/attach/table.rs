// Derived from herdr-core crates/herdr-core/src/server.rs and wire.rs (Apache-2.0).

//! The shared connection's channel table and attach-call table.
//!
//! [`Channels`] is the routing state for every attach on one connection:
//! `Live` slots for installed attaches, `Held` slots for frames that
//! arrive between the attach request and its reply's install, and `Dead`
//! slots that swallow late frames for closed channels. Routing never
//! awaits: a stalled `recv` only fills its own bounded backlog, so no
//! channel can delay another or a channel-0 reply.
//!
//! [`AttachCalls`] is the `terminal.attach` in-flight table: unlike a
//! pending RPC entry, an entry is never removed when its caller goes
//! away — the reply names a channel the core already allocated, so a
//! late reply must still be read to get its channel CLOSED instead of
//! leaking the viewer.

use super::pump::DataQueue;
use super::stream::Stream;
use crate::client::error::CallError;
use crate::client::lock;
use crate::frame::Frame;
use crate::jsonrpc::Id;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

/// Payload bytes held for one not-yet-installed channel: the same window
/// a dedicated stream allowed before install.
const HELD_BYTES: usize = crate::frame::INITIAL_CREDIT as usize + 64 * 1024;
/// Frames held for one not-yet-installed channel.
const HELD_FRAMES: usize = 1024;

/// `terminal.attach` calls whose reply may still land. Entries outlive
/// their callers so a cancelled open's late reply still names a channel
/// that gets CLOSED — the viewer is never leaked.
#[derive(Default)]
pub(super) struct AttachCalls {
    inner: Mutex<AttachCallsInner>,
}

#[derive(Default)]
struct AttachCallsInner {
    map: HashMap<Id, oneshot::Sender<Result<Value, CallError>>>,
    /// Connection dead: set inside `fail_all` while the map is locked, so
    /// no call can register after the sweep and hang.
    dead: bool,
}

impl AttachCalls {
    /// Registers `id`: `Closed` when the connection is dead (checked under
    /// the map lock, atomic with `fail_all`), `Full` at the in-flight bound.
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

    /// Whether `id` is an in-flight attach call.
    pub(super) fn contains(&self, id: &Id) -> bool {
        lock(&self.inner).map.contains_key(id)
    }

    /// Takes the sender for `id`'s reply.
    pub(super) fn take(&self, id: &Id) -> Option<oneshot::Sender<Result<Value, CallError>>> {
        lock(&self.inner).map.remove(id)
    }

    /// Fails every in-flight attach call and latches the table dead —
    /// under the map lock, so a later `register` sees `dead` and can
    /// never register into a dead connection.
    pub(super) fn fail_all(&self, reason: Option<String>) {
        let senders = {
            let mut inner = lock(&self.inner);
            inner.dead = true;
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

/// One channel-table slot.
enum Slot {
    /// Frames arrived before the attach reply installed this channel —
    /// replayed in order at install; bounded so a runaway held channel
    /// is a connection-level violation, exactly like a dedicated wire's.
    Held {
        frames: VecDeque<Frame>,
        bytes: usize,
    },
    /// An installed attach.
    Live {
        stream: Arc<Stream>,
        data: Arc<DataQueue>,
    },
    /// Ended — close acknowledged or torn down; late frames drop here.
    Dead,
}

/// What `route` decided for one inbound stream frame.
pub(super) enum Route {
    /// Routed.
    Ok,
    /// A channel-scoped violation: the stream is failed and its slot is
    /// dead — send this channel a CLOSE. The connection survives.
    CloseChannel(u32),
    /// A connection-level violation: kill the whole wire.
    Fatal(String),
}

/// The result `install` reports for a replied channel.
pub(super) enum Install {
    /// Installed; held frames replayed.
    Ok,
    /// The connection died before install ran.
    Dead,
    /// A channel-scoped violation — the slot is already `Dead`; the
    /// caller closes the channel and fails this open.
    Violation(String),
}

/// The channel table: routing state for every attach on one connection.
/// All mutation happens under this one lock, so install's held replay is
/// atomic against a concurrently arriving frame.
#[derive(Default)]
pub(super) struct Channels {
    map: HashMap<u32, Slot>,
    /// Connection dead: set inside `fail_all` while the map is locked.
    dead: bool,
}

impl Channels {
    /// Routes one inbound stream frame. Never awaits: a stalled consumer
    /// only fills its own bounded backlog — overflow ends that channel,
    /// never the reader.
    pub(super) fn route(&mut self, frame: Frame) -> Route {
        let channel = frame.channel;
        match self.map.entry(channel) {
            std::collections::hash_map::Entry::Vacant(e) => {
                let bytes = frame.payload.len();
                let mut frames = VecDeque::new();
                frames.push_back(frame);
                e.insert(Slot::Held { frames, bytes });
                Route::Ok
            }
            std::collections::hash_map::Entry::Occupied(mut e) => match e.get_mut() {
                Slot::Dead => Route::Ok,
                Slot::Held { frames, bytes } => {
                    *bytes += frame.payload.len();
                    if *bytes > HELD_BYTES || frames.len() >= HELD_FRAMES {
                        return Route::Fatal(format!(
                            "held-frame bound exceeded on channel {channel}"
                        ));
                    }
                    frames.push_back(frame);
                    Route::Ok
                }
                Slot::Live { stream, data } => {
                    if let Err(why) = stream.push(frame) {
                        // A channel's own violation — a bad frame or a
                        // credit-window overflow — ends only this attach.
                        stream.fail(Some(why));
                        data.close();
                        e.insert(Slot::Dead);
                        return Route::CloseChannel(channel);
                    }
                    if stream.ended() {
                        data.close();
                        e.insert(Slot::Dead);
                    }
                    Route::Ok
                }
            },
        }
    }

    /// Installs the channel the attach reply named and replays its held
    /// frames — all under the table lock, so a frame arriving during
    /// install can never jump ahead of older held ones.
    pub(super) fn install(
        &mut self,
        channel: u32,
        stream: Arc<Stream>,
        data: Arc<DataQueue>,
    ) -> Install {
        if self.dead {
            return Install::Dead;
        }
        if let Err(why) = stream.install(channel) {
            return Install::Violation(why);
        }
        let held = match self.map.entry(channel) {
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(Slot::Live {
                    stream: Arc::clone(&stream),
                    data: Arc::clone(&data),
                });
                VecDeque::new()
            }
            std::collections::hash_map::Entry::Occupied(mut e) => match e.get_mut() {
                Slot::Live {
                    stream: old_stream,
                    data: old_data,
                } => {
                    // The core re-issued a channel it never closed: the
                    // old attach cannot keep routing, so it ends too —
                    // the violation still cannot take down the wire.
                    old_stream.fail(Some(format!("channel {channel} was reissued")));
                    old_data.close();
                    e.insert(Slot::Dead);
                    return Install::Violation(format!("channel {channel} was already attached"));
                }
                Slot::Held { frames, .. } => {
                    let held = std::mem::take(frames);
                    e.insert(Slot::Live {
                        stream: Arc::clone(&stream),
                        data: Arc::clone(&data),
                    });
                    held
                }
                Slot::Dead => {
                    // The core reused a closed channel number.
                    e.insert(Slot::Live {
                        stream: Arc::clone(&stream),
                        data: Arc::clone(&data),
                    });
                    VecDeque::new()
                }
            },
        };
        for frame in held {
            if let Err(why) = stream.push(frame) {
                stream.fail(Some(why.clone()));
                data.close();
                self.map.insert(channel, Slot::Dead);
                return Install::Violation(why);
            }
        }
        if stream.ended() {
            data.close();
            self.map.insert(channel, Slot::Dead);
        }
        Install::Ok
    }

    /// Marks one channel dead: client-side teardown (detach/drop) or a
    /// late attach reply for a cancelled open. Held frames drop; a live
    /// attach's own teardown already failed its stream.
    pub(super) fn end(&mut self, channel: u32) {
        self.map.insert(channel, Slot::Dead);
    }

    /// Ends every channel with the connection's own end reason — each
    /// attach sees the same failure shape it would on a dedicated wire.
    pub(super) fn fail_all(&mut self, reason: Option<String>) {
        self.dead = true;
        for (_, slot) in self.map.drain() {
            if let Slot::Live { stream, data } = slot {
                stream.fail(reason.clone());
                data.close();
            }
        }
    }

    /// Every live `(stream, data)` pair — a snapshot for the writer so it
    /// never holds the table lock across a transport write.
    pub(super) fn live(&self) -> Vec<(Arc<Stream>, Arc<DataQueue>)> {
        self.map
            .values()
            .filter_map(|slot| match slot {
                Slot::Live { stream, data } => Some((Arc::clone(stream), Arc::clone(data))),
                _ => None,
            })
            .collect()
    }

    /// Every live input queue — for `mark_dead` when the writer exits.
    pub(super) fn datas(&self) -> Vec<Arc<DataQueue>> {
        self.map
            .values()
            .filter_map(|slot| match slot {
                Slot::Live { data, .. } => Some(Arc::clone(data)),
                _ => None,
            })
            .collect()
    }

    /// Every live queue is closed and no credit is owed — the writer's
    /// drain-complete check once the control queue also ends.
    pub(super) fn drained(&self) -> bool {
        self.map.values().all(|slot| match slot {
            Slot::Live { stream, data } => data.closed() && stream.owed() == 0,
            _ => true,
        })
    }
}

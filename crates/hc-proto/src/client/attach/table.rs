// Derived from herdr-core crates/herdr-core/src/server.rs and wire.rs (Apache-2.0).

//! The shared connection's channel table.
//!
//! [`Channels`] is the routing state for every attach on one connection:
//! `Live` slots for installed attaches, `Held` slots for frames that
//! arrive between the attach request and its reply's install, and `Dead`
//! slots that drop late frames for client-closed channels until the
//! core's CLOSE arrives — then the slot retires entirely, and a small
//! bounded tombstone keeps stragglers dropping instead of reviving a
//! `Held` slot. Routing never awaits: a stalled `recv` only fills its
//! own bounded backlog, so no channel can delay another or a channel-0
//! reply.

use super::pump::DataQueue;
use super::stream::Stream;
use crate::frame::{Frame, Kind};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

/// Payload bytes held for one not-yet-installed channel: the same window
/// a dedicated stream allowed before install.
const HELD_BYTES: usize = crate::frame::INITIAL_CREDIT as usize + 64 * 1024;
/// Frames held for one not-yet-installed channel.
const HELD_FRAMES: usize = 1024;
/// `Dead` slots kept while their CLOSE ack is still out: the client
/// closes burst over the core's echo cadence by this much before the
/// oldest unacknowledged one retires anyway.
const CLOSING_BOUND: usize = 64;
/// Retired channels remembered so stragglers still drop. One resize
/// burst is far beyond the reordering a single TCP stream can produce —
/// older stragglers degrade to `Held` and are replayed out harmlessly.
const CLOSED_BOUND: usize = 256;

/// One channel-table slot.
enum Slot {
    /// Attach `install` not run yet — the reply arrived but the open is
    /// still completing — so frames queue up to the held bound.
    Held {
        frames: VecDeque<Frame>,
        bytes: usize,
    },
    /// Attach installed: frames flow to its queue.
    Live {
        stream: Arc<Stream>,
        data: Arc<DataQueue>,
    },
    /// The channel is over client-side but the core's CLOSE has not
    /// landed yet: late frames still drop here instead of becoming
    /// `Held` for a channel nobody will ever attach.
    Dead,
}

/// A bounded FIFO set of channel numbers: insertion past `bound` evicts
/// and returns the oldest member.
struct FifoSet {
    order: VecDeque<u32>,
    set: HashSet<u32>,
    bound: usize,
}

impl FifoSet {
    fn new(bound: usize) -> FifoSet {
        FifoSet {
            order: VecDeque::new(),
            set: HashSet::new(),
            bound,
        }
    }

    /// Inserts `channel`; `Some(evicted)` when the bound pushed the
    /// oldest member out. Stale order entries are skipped.
    fn insert(&mut self, channel: u32) -> Option<u32> {
        if !self.set.insert(channel) {
            return None;
        }
        self.order.push_back(channel);
        while self.set.len() > self.bound {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if self.set.remove(&oldest) {
                return Some(oldest);
            }
        }
        None
    }

    /// Removes `channel` — order and set stay consistent.
    fn remove(&mut self, channel: u32) -> bool {
        if !self.set.remove(&channel) {
            return false;
        }
        if let Some(at) = self.order.iter().position(|c| *c == channel) {
            self.order.remove(at);
        }
        true
    }

    /// Whether `channel` is a member.
    fn contains(&self, channel: u32) -> bool {
        self.set.contains(&channel)
    }
}

/// What the reader does with a frame it could not route.
pub(super) enum Route {
    /// Routed — delivered, or dropped for a `Dead` slot, or held.
    Ok,
    /// `Dead` was reached through a live-slot violation — send CLOSE.
    CloseChannel(u32),
    /// The frame broke the protocol: tear down the whole connection.
    Fatal(String),
}

/// What `install` did with the channel its reply named.
pub(super) enum Install {
    /// The channel is live — held frames, if any, were replayed.
    Ok,
    /// The connection died first — install elsewhere.
    Dead,
    /// The reply violated the protocol — a channel already attached or
    /// frames a fresh stream cannot take.
    Violation(String),
}

/// Every channel on the connection. Looked up for inbound frames and
/// torn down piecemeal as attaches detach or get dropped.
pub(super) struct Channels {
    map: HashMap<u32, Slot>,
    /// `Dead` slots by age — client-closed channels still waiting for
    /// the core's CLOSE, bounded by [`CLOSING_BOUND`].
    closing: FifoSet,
    /// Channels whose close completed — a bounded tombstone so late
    /// frames still drop instead of reviving a `Held` slot.
    closed: FifoSet,
    /// Connection dead: set inside `fail_all` while the map is locked.
    dead: bool,
}

impl Default for Channels {
    fn default() -> Channels {
        Channels {
            map: HashMap::new(),
            closing: FifoSet::new(CLOSING_BOUND),
            closed: FifoSet::new(CLOSED_BOUND),
            dead: false,
        }
    }
}

impl Channels {
    /// Routes one inbound stream frame. Never awaits: a stalled consumer
    /// only fills its own bounded backlog — overflow ends that channel,
    /// never the reader.
    pub(super) fn route(&mut self, frame: Frame) -> Route {
        let channel = frame.channel;
        // Table side effects deferred until the entry borrow ends.
        let mut retire = false;
        let mut pending = false;
        let mut route = Route::Ok;
        match self.map.entry(channel) {
            std::collections::hash_map::Entry::Vacant(e) => {
                if self.closed.contains(channel) {
                    // A straggler for a fully closed channel: drop it
                    // rather than holding frames for a channel nobody
                    // will ever attach.
                    return Route::Ok;
                }
                let bytes = frame.payload.len();
                let mut frames = VecDeque::new();
                frames.push_back(frame);
                e.insert(Slot::Held { frames, bytes });
            }
            std::collections::hash_map::Entry::Occupied(mut e) => match e.get_mut() {
                Slot::Dead => {
                    // The core's CLOSE for a pending close retires the slot.
                    retire = frame.kind == Kind::Close;
                }
                Slot::Held { frames, bytes } => {
                    *bytes += frame.payload.len();
                    if *bytes > HELD_BYTES || frames.len() >= HELD_FRAMES {
                        return Route::Fatal(format!(
                            "held-frame bound exceeded on channel {channel}"
                        ));
                    }
                    frames.push_back(frame);
                }
                Slot::Live { stream, data } => {
                    if let Err(why) = stream.push(frame) {
                        // A channel's own violation — a bad frame or a
                        // credit-window overflow — ends only this attach.
                        stream.fail(Some(why));
                        data.close();
                        e.insert(Slot::Dead);
                        pending = true;
                        route = Route::CloseChannel(channel);
                    } else if stream.ended() {
                        // The core's CLOSE arrived: retire the slot.
                        data.close();
                        retire = true;
                    }
                }
            },
        }
        if retire {
            self.retire(channel);
        }
        if pending {
            self.mark_closing(channel);
        }
        route
    }

    /// Installs a fresh attach on `channel` — the channel-0 reply named
    /// it, so a `Held` slot's buffered frames replay into the stream.
    /// `Violation` for a `Live` clash or a replay failure: the new
    /// attach fails, its channel slot is dead, and the wire goes on —
    /// parity with `Attach::open` on a dedicated wire, where the same
    /// violations tear only the one attach down.
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
        let mut pending = false;
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
                    pending = true;
                    VecDeque::new()
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
                    // The core reused a channel number whose close
                    // was still out: the old channel ends live.
                    e.insert(Slot::Live {
                        stream: Arc::clone(&stream),
                        data: Arc::clone(&data),
                    });
                    VecDeque::new()
                }
            },
        };
        // The number is live again (or freshly dead): its earlier close
        // bookkeeping is stale and its late CLOSE will route live.
        self.closing.remove(channel);
        self.closed.remove(channel);
        if pending {
            self.mark_closing(channel);
            return Install::Violation(format!("channel {channel} was already attached"));
        }
        for frame in held {
            if let Err(why) = stream.push(frame) {
                stream.fail(Some(why.clone()));
                data.close();
                self.map.insert(channel, Slot::Dead);
                self.mark_closing(channel);
                return Install::Violation(why);
            }
        }
        if stream.ended() {
            // The replayed CLOSE already ended it — the core's close
            // landed, so the slot retires like any other closed channel.
            data.close();
            self.retire(channel);
        }
        Install::Ok
    }

    /// Marks one channel dead: client-side teardown (detach/drop) or a
    /// late attach reply for a cancelled open. Held frames drop; a live
    /// attach's own teardown already failed its stream. The slot stays
    /// `Dead` — dropping late frames — until the core's CLOSE retires
    /// it; a channel whose close already landed, or a dead table, keeps
    /// no slot at all.
    pub(super) fn end(&mut self, channel: u32) {
        if self.dead || self.closed.contains(channel) {
            return;
        }
        if let Some(Slot::Dead) = self.map.get(&channel) {
            return;
        }
        self.map.insert(channel, Slot::Dead);
        self.mark_closing(channel);
    }

    /// Table entries — live, held and dead slots together.
    pub(super) fn len(&self) -> usize {
        self.map.len()
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

    /// The core's CLOSE for `channel` landed: the slot leaves the map
    /// entirely and a tombstone keeps stragglers dropping, so the map
    /// never grows with the number of closed channels.
    fn retire(&mut self, channel: u32) {
        self.map.remove(&channel);
        self.closing.remove(channel);
        self.closed.insert(channel);
    }

    /// `channel`'s client CLOSE went out — its `Dead` slot drops late
    /// frames until the core's CLOSE arrives. The pending record is
    /// bounded: the oldest unacknowledged slot retires early.
    fn mark_closing(&mut self, channel: u32) {
        if let Some(evicted) = self.closing.insert(channel) {
            self.map.remove(&evicted);
            self.closed.insert(evicted);
        }
    }
}

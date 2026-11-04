// Derived from herdr-core crates/herdr-core/src/remote.rs (apply, mirror_event)
// and schema/README.md (Apache-2.0).

//! The terminal inventory one `events.subscribe` stream describes, as a
//! pure reducer: snapshots initialize or wholesale-replace it, then
//! unfiltered event sequence advances it. No async, no RPC, no UI — the
//! watcher (a later task) reads [`Effect`] flags to schedule its own
//! refreshes.
//!
//! Sequencing is the *unfiltered* contract: `seq` rises by one per event,
//! in order, so a gap — or a heartbeat ahead of what was applied — means
//! events were lost and the inventory is stale until a snapshot replaces
//! it. A *filtered* stream need not receive every `seq`: gaps are then
//! expected and are not reported. A `missed` snapshot always replaces the
//! whole inventory, even at the same `seq`, and always means "refetch
//! documents": snapshots contain terminals only. A new connection starts a
//! fresh sequence epoch — `replace` resets both the map and `seq`.
//!
//! Status is never inferred from terminal text or processes: only what the
//! events themselves say is applied. Documented kinds are validated
//! atomically before a mutation — a malformed known event is never
//! silently advanced past as healthy; it marks the inventory stale and
//! reports a resync, leaving `seq` and records untouched. Unknown kinds
//! still advance the sequence normally.

mod kinds;

use crate::events::{EventParams, HeartbeatParams, MissedParams, SubscribeParams};
use crate::terminal::Terminal;
use std::collections::BTreeMap;

/// A document a `doc` event says to refetch: its name, and the machine the
/// change happened on when the core names one (`on` for `doc.get`).
#[derive(Debug, Clone, PartialEq)]
pub struct DocRef {
    /// The event's `machine` — this core's name for a machine it added;
    /// `None` for the local core's own documents.
    pub machine: Option<String>,
    /// The document name from the event's `data`.
    pub name: String,
}

/// What one applied notification changed, for the watcher to act on.
/// Lists hold terminal addresses; `docs` holds refetch targets.
#[derive(Debug, Default, PartialEq)]
pub struct Effect {
    /// Records added: `created`, a `labels` event carrying a whole
    /// `record`, or snapshot newcomers.
    pub created: Vec<String>,
    /// Records wholly replaced after a loss — reattach candidates.
    pub restarted: Vec<String>,
    /// Records that left the inventory (`removed`, a label filter's
    /// non-match, or snapshot disappearance).
    pub removed: Vec<String>,
    /// Records whose fields changed (any partial apply or a snapshot diff).
    pub changed: Vec<String>,
    /// Records whose grid size changed (a subset of `changed`).
    pub resized: Vec<String>,
    /// Documents to refetch, from `doc` events.
    pub docs: Vec<DocRef>,
    /// The inventory was wholesale-replaced by a snapshot (initial,
    /// `missed` or a new epoch): refetch *all* watched documents.
    pub snapshot: bool,
    /// An unfiltered sequence gap was crossed, or a documented event was
    /// malformed and dropped: the inventory is stale until the next
    /// `replace`.
    pub resync: bool,
    /// A *documented* event kind arrived malformed — required members
    /// missing or mistyped, a `terminal` absent, a whole record that does
    /// not fit, or a `terminal`/`record.address` mismatch. The event was
    /// dropped without touching the map and `seq` was not advanced; a
    /// resync refetches the real state. Unknown kinds never set this.
    pub malformed: bool,
}

impl Effect {
    /// Nothing changed or needs doing.
    pub fn is_empty(&self) -> bool {
        self == &Effect::default()
    }
}

/// A pure terminal inventory: keyed by address, replaced by snapshots,
/// advanced by events. Query it freely; mutation goes only through
/// `replace`/`apply`/`heartbeat`/`missed`.
#[derive(Debug, Default, Clone)]
pub struct Inventory {
    pub(super) terminals: BTreeMap<String, Terminal>,
    /// Highest applied `seq` in this epoch.
    seq: u64,
    /// The subscription filtered: `seq` gaps are expected, not resyncs.
    filtered: bool,
    /// A subscription label filter: an event leaving it unsatisfied
    /// removes the record from the view, as core's contract documents.
    pub(super) label_filter: Option<BTreeMap<String, String>>,
    /// A detected gap or malformed drop: `true` until a snapshot replaces
    /// the inventory.
    stale: bool,
    /// Marked offline by the owner; the last inventory is kept as-is.
    offline: bool,
}

impl Inventory {
    /// An unfiltered inventory: every `seq` arrives in order.
    pub fn unfiltered() -> Inventory {
        Inventory::default()
    }

    /// An inventory matching the subscription's filters. Any filter makes
    /// `seq` gaps legal; a label filter additionally removes records whose
    /// reported labels stop matching it.
    pub fn for_subscription(params: &SubscribeParams) -> Inventory {
        Inventory {
            filtered: params.terminal.is_some()
                || params.children_of.is_some()
                || !params.labels.is_empty(),
            label_filter: (!params.labels.is_empty()).then(|| params.labels.clone()),
            ..Inventory::default()
        }
    }

    /// Every record by address.
    pub fn terminals(&self) -> &BTreeMap<String, Terminal> {
        &self.terminals
    }

    /// One record by address, e.g. `t7` or `host-b/t7`.
    pub fn get(&self, address: &str) -> Option<&Terminal> {
        self.terminals.get(address)
    }

    /// The highest applied `seq` in this epoch.
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// A gap or a malformed drop needs a snapshot before this is current.
    pub fn is_stale(&self) -> bool {
        self.stale
    }

    /// The subscription filtered its stream (see [`filtered`](Self::filtered)).
    pub fn filtered(&self) -> bool {
        self.filtered
    }

    /// The owner lost the stream: mark it offline. The last inventory is
    /// preserved unchanged — connection policy is the watcher's task, not
    /// the reducer's.
    pub fn set_offline(&mut self, offline: bool) {
        self.offline = offline;
    }

    /// Whether the owner marked the inventory offline.
    pub fn is_offline(&self) -> bool {
        self.offline
    }

    /// Wholesale replacement — the initial snapshot, a `missed` one, or a
    /// new epoch after reconnect. Always replaces, even at the same `seq`;
    /// reports what appeared, changed and disappeared, and clears `stale`.
    /// The `snapshot` flag means documents must be refetched: snapshots
    /// never carry them.
    pub fn replace(&mut self, seq: u64, terminals: Vec<Terminal>) -> Effect {
        let fresh: BTreeMap<String, Terminal> = terminals
            .into_iter()
            .map(|t| (t.address.clone(), t))
            .collect();
        let mut effect = Effect {
            snapshot: true,
            ..Effect::default()
        };
        for address in self.terminals.keys() {
            if !fresh.contains_key(address) {
                effect.removed.push(address.clone());
            }
        }
        for (address, record) in &fresh {
            match self.terminals.get(address) {
                None => effect.created.push(address.clone()),
                Some(old) if old != record => effect.changed.push(address.clone()),
                _ => {}
            }
        }
        self.terminals = fresh;
        self.seq = seq;
        self.stale = false;
        effect
    }

    /// Advances the inventory by one event. Stale or duplicate `seq`s do
    /// not apply. A malformed *documented* event is dropped untouched —
    /// the map keeps its last good state, `seq` does not advance, and the
    /// effect reports `resync` + `malformed` instead of presenting the
    /// inventory as current. Unknown kinds validate free and advance the
    /// sequence normally; a gap on an unfiltered stream sets `stale` and
    /// reports `resync` while the event itself still applies.
    pub fn apply(&mut self, event: &EventParams) -> Effect {
        let mut effect = Effect::default();
        if event.seq <= self.seq {
            return effect;
        }
        if kinds::check(event).is_err() {
            self.stale = true;
            effect.resync = true;
            effect.malformed = true;
            return effect;
        }
        if event.seq > self.seq + 1 && !self.filtered {
            self.stale = true;
            effect.resync = true;
        }
        self.seq = event.seq;
        self.apply_event(event, &mut effect);
        effect
    }

    /// A `heartbeat` carries the core's current `seq`: ahead of what was
    /// applied on an unfiltered stream, it means events were lost.
    pub fn heartbeat(&mut self, heartbeat: &HeartbeatParams) -> Effect {
        if heartbeat.seq > self.seq && !self.filtered {
            self.stale = true;
            return Effect {
                resync: true,
                ..Effect::default()
            };
        }
        Effect::default()
    }

    /// A `missed` notification is a snapshot: always a wholesale replace
    /// (even at the same `seq`), always a document-refetch signal.
    pub fn missed(&mut self, missed: &MissedParams) -> Effect {
        self.replace(missed.seq, missed.terminals.clone())
    }
}

// Derived from herdr src/client/shell/endpoints.rs (Apache-2.0).
//! Bounded display receipts allocated before the watcher coalesces views.
use crate::inventory::Inventory;
use crate::terminal::Terminal;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Clock {
    domain: (String, String, u64),
    ordinal: Option<u64>,
}

impl Clock {
    fn of(term: &Terminal) -> Option<Self> {
        let activity = term.activity.as_ref()?;
        let ordinal = activity
            .records
            .iter()
            .filter(|(id, _)| id.is_empty() || term.records.contains_key(*id))
            .flat_map(|(_, clock)| [clock.state_ordinal, clock.completion_ordinal])
            .flatten()
            .max();
        Some(Self {
            domain: (
                activity.core_id.clone(),
                activity.epoch.clone(),
                activity.scope,
            ),
            ordinal,
        })
    }
}

#[derive(Clone, Debug)]
struct Observation {
    clock: Option<Clock>,
    created_ms: u64,
    receipt: u64,
}

/// Local presentation metadata, never a core fact or completion authority.
/// Only current live reporters are retained; there is no lifetime counter/log.
#[derive(Clone, Debug, Default)]
pub struct ActivityReceipts {
    observed: BTreeMap<String, Observation>,
}

impl ActivityReceipts {
    /// A stable sorting key; equal saturated receipts keep traversal order.
    pub fn rank(&self, address: &str) -> u64 {
        self.observed.get(address).map_or(0, |o| o.receipt)
    }

    /// Ingest a complete authoritative state before consumers coalesce it.
    pub fn ingest(&mut self, inventory: &Inventory) {
        if inventory.is_stale() || inventory.is_offline() {
            return;
        }
        let terms = inventory
            .ordered_terminals()
            .filter(|term| term.is_live_reporter())
            .collect::<Vec<_>>();
        // Reference computes the maximum before pruning the incoming snapshot.
        let mut next = self.observed.values().map(|o| o.receipt).max().unwrap_or(0);
        let mut domains = BTreeMap::<_, Vec<usize>>::new();
        let mut pending = Vec::new();
        for term in &terms {
            let clock = Clock::of(term);
            if let Some(previous) = self.observed.get(&term.address)
                && previous.created_ms == term.created_ms
            {
                if previous.clock == clock {
                    continue;
                }
                if previous.clock.as_ref().map(|c| &c.domain) != clock.as_ref().map(|c| &c.domain) {
                    // Preserve the existing lifetime baseline. Reference-compatible
                    // epoch display rebuilding is a separate, pending patch.
                    self.observed.insert(
                        term.address.clone(),
                        Observation {
                            clock,
                            ..previous.clone()
                        },
                    );
                    continue;
                }
            }
            if let Some(clock) = &clock
                && clock.ordinal.is_some()
            {
                domains
                    .entry(clock.domain.clone())
                    .or_default()
                    .push(pending.len());
            }
            pending.push((*term, clock));
        }
        // Comparable members sort only into their own incoming slots. Other
        // domains/unknown history keep their slot order, never a foreign sort key.
        for slots in domains.values() {
            let mut batch = slots
                .iter()
                .map(|index| pending[*index].clone())
                .collect::<Vec<_>>();
            batch.sort_by_key(|(_, clock)| clock.as_ref().and_then(|c| c.ordinal));
            for (index, item) in slots.iter().zip(batch) {
                pending[*index] = item;
            }
        }
        for (term, clock) in pending {
            next = next.saturating_add(1);
            self.observed.insert(
                term.address.clone(),
                Observation {
                    clock,
                    created_ms: term.created_ms,
                    receipt: next,
                },
            );
        }
        self.observed.retain(|address, _| {
            inventory
                .get(address)
                .is_some_and(Terminal::is_live_reporter)
        });
    }
}

#[cfg(test)]
#[path = "receipts_tests.rs"]
mod tests;

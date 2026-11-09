// Derived from herdr src/client/shell/endpoints.rs (Apache-2.0).
//! Bounded display receipts allocated before the watcher coalesces views.
use crate::inventory::Inventory;
use crate::machine::MachineView;
use crate::terminal::Terminal;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Clock {
    domain: (String, String, u64),
    ordinal: Option<u64>,
}

impl Clock {
    fn of(term: &Terminal) -> Option<Self> {
        let Some(activity) = term.activity.as_ref() else {
            // Without `record_activity` the only authoritative change
            // marker is `turn` — a counter private to this terminal, so it
            // rides in a singleton domain keyed by the address: its bumps
            // still reallocate the display receipt but the ordinal can
            // never sort against a foreign counter.
            return Some(Self {
                domain: (term.address.clone(), String::new(), 0),
                ordinal: Some(term.turn),
            });
        };
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

/// The endpoint part of an address: `host-b` of `host-b/t7`, `""` for a
/// bare local terminal — the `machine_of` contract the UI orders by.
fn endpoint_of(address: &str) -> &str {
    address.split_once('/').map_or("", |(prefix, _)| prefix)
}

/// Whether the record backing a baseline id is still present: the root id
/// tracks `status`, named ids track `records` membership.
fn baseline_source(term: &Terminal, id: &str) -> bool {
    if id.is_empty() {
        term.status.is_some()
    } else {
        term.records.contains_key(id)
    }
}

/// The displayable record completions a term carried when its current
/// activity domain was entered. Mirrors the completion projection in the
/// presentation layer: empty-record status comes from the terminal itself,
/// named records from the record table.
fn completions_at(term: &Terminal) -> BTreeMap<String, u64> {
    term.activity
        .as_ref()
        .into_iter()
        .flat_map(|a| &a.records)
        .filter_map(|(id, clock)| {
            let status = if id.is_empty() {
                term.status.as_ref()
            } else {
                term.records.get(id)
            }?;
            matches!(status.state.as_str(), "idle" | "done")
                .then_some((id.clone(), clock.completion_ordinal?))
        })
        .collect()
}

#[derive(Clone, Debug)]
struct Observation {
    clock: Option<Clock>,
    created_ms: u64,
    receipt: u64,
    /// `Some` for the arrivals the UI may baseline: the completions the
    /// current lifetime's (re)establishment arrived with. A first sight
    /// into an epoch its origin already ingested is ineligible — `None`.
    epoch_completions: Option<BTreeMap<String, u64>>,
    /// Serial bumped on every lifetime (re)establishment — boundary, first
    /// sight or incarnation — so Some(A) -> None -> Some(A) is still a
    /// boundary when only the final state is consumed.
    epoch_token: u64,
}

/// Local presentation metadata, never a core fact or completion authority.
/// Only current live reporters are retained; there is no lifetime counter/log.
#[derive(Clone, Debug, Default)]
pub struct ActivityReceipts {
    observed: BTreeMap<String, Observation>,
    /// Allocation serial for `Observation::epoch_token`.
    boundary_seq: u64,
    /// One current `(core_id, epoch)` slot per endpoint, updated by every
    /// authoritative ingest, retained while the endpoint has zero agents
    /// and retired when the endpoint leaves the machine set — never a
    /// history. Eligibility matches the presentation's `(core_id, epoch)`
    /// key: scope is per terminal/incarnation.
    endpoint_epochs: BTreeMap<String, (String, String)>,
}

impl ActivityReceipts {
    /// A stable sorting key; equal saturated receipts keep traversal order.
    pub fn rank(&self, address: &str) -> u64 {
        self.observed.get(address).map_or(0, |o| o.receipt)
    }

    /// The completions the current lifetime's establishment arrived with —
    /// `None` for agents whose arrival was not baseline-eligible. A genuine
    /// boundary reallocates the receipt but this baseline keeps later real
    /// work visible even when intermediate views are never drawn.
    pub fn epoch_completions(&self, address: &str) -> Option<&BTreeMap<String, u64>> {
        self.observed
            .get(address)
            .and_then(|o| o.epoch_completions.as_ref())
    }

    /// The ingestion serial of the current lifetime's establishment —
    /// distinguishes Some(A) -> None -> Some(A) before consumption where a
    /// lifetime equality check cannot.
    pub fn epoch_token(&self, address: &str) -> Option<u64> {
        self.observed.get(address).map(|o| o.epoch_token)
    }

    /// Retire endpoint slots whose machine left the watcher's set. `None`
    /// — lifecycle unknown — leaves slots alone; the local endpoint `""`
    /// is always live.
    pub fn sync_machines(&mut self, machines: Option<&[MachineView]>) {
        let Some(machines) = machines else { return };
        self.endpoint_epochs.retain(|endpoint, _| {
            endpoint.is_empty() || machines.iter().any(|m| m.name == *endpoint)
        });
    }

    /// Ingest a complete authoritative state before consumers coalesce it.
    /// `traversal` is the resolved UI draw order: boundary-class entries
    /// allocate by traversal index, untraversed ones fall back to incoming
    /// order; everything else keeps the incoming-slot discipline.
    pub fn ingest(&mut self, inventory: &Inventory, traversal: &[String]) {
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
            let unchanged = self
                .observed
                .get(&term.address)
                .is_some_and(|p| p.created_ms == term.created_ms && p.clock == clock);
            if unchanged {
                // Withdrawn baseline ids leave the boundary baseline on every
                // authoritative ingest — even an unchanged clock. The root id
                // tracks `status` presence, named ids track `records`
                // membership, matching `completions_at`.
                let previous = self.observed.get_mut(&term.address).unwrap();
                if let Some(seen) = previous.epoch_completions.as_mut() {
                    seen.retain(|id, _| baseline_source(term, id));
                }
                continue;
            }
            let mut epoch_token = None;
            let mut boundary = false;
            let epoch_completions = match self.observed.get(&term.address) {
                Some(previous) if previous.created_ms == term.created_ms => {
                    if previous.clock.as_ref().map(|c| &c.domain)
                        == clock.as_ref().map(|c| &c.domain)
                    {
                        // Same domain, new ordinals: the boundary baseline
                        // and token survive, pruned to the sources present.
                        epoch_token = Some(previous.epoch_token);
                        previous.epoch_completions.clone().map(|mut seen| {
                            seen.retain(|id, _| baseline_source(term, id));
                            seen
                        })
                    } else {
                        // A genuine epoch boundary reallocates the display
                        // receipt and baselines the completions it arrived with.
                        boundary = true;
                        Some(completions_at(term))
                    }
                }
                previous => {
                    // A boundary or new incarnation is always eligible; a
                    // first sight is eligible only into an epoch its
                    // endpoint's current origin never ingested — never just
                    // "first time drawn".
                    boundary = previous.is_some();
                    let eligible = previous.is_some()
                        || clock.as_ref().is_none_or(|c| {
                            self.endpoint_epochs
                                .get(endpoint_of(&term.address))
                                .is_none_or(|(core, epoch)| {
                                    core != &c.domain.0 || epoch != &c.domain.1
                                })
                        });
                    eligible.then(|| completions_at(term))
                }
            };
            let epoch_token = epoch_token.unwrap_or_else(|| {
                self.boundary_seq = self.boundary_seq.saturating_add(1);
                self.boundary_seq
            });
            if let Some(clock) = &clock
                && clock.ordinal.is_some()
            {
                domains
                    .entry(clock.domain.clone())
                    .or_default()
                    .push(pending.len());
            }
            pending.push((*term, clock, epoch_completions, epoch_token, boundary));
        }
        // Comparable members sort only into their own incoming slots. Other
        // domains/unknown history keep their slot order, never a foreign sort key.
        for slots in domains.values() {
            let mut batch = slots
                .iter()
                .map(|index| pending[*index].clone())
                .collect::<Vec<_>>();
            batch.sort_by_key(|(_, clock, _, _, _)| clock.as_ref().and_then(|c| c.ordinal));
            for (index, item) in slots.iter().zip(batch) {
                pending[*index] = item;
            }
        }
        // Boundary-class entries order by the resolved traversal among their
        // own positions; untraversed entries keep incoming order (stable, last).
        if !traversal.is_empty() {
            let order: BTreeMap<&str, usize> = traversal
                .iter()
                .enumerate()
                .map(|(index, address)| (address.as_str(), index))
                .collect();
            let positions = (0..pending.len())
                .filter(|index| pending[*index].4)
                .collect::<Vec<_>>();
            let mut batch = positions
                .iter()
                .map(|index| pending[*index].clone())
                .collect::<Vec<_>>();
            batch.sort_by_key(|entry| {
                order
                    .get(entry.0.address.as_str())
                    .copied()
                    .unwrap_or(usize::MAX)
            });
            for (index, entry) in positions.into_iter().zip(batch) {
                pending[index] = entry;
            }
        }
        for (term, clock, epoch_completions, epoch_token, _) in pending {
            next = next.saturating_add(1);
            self.observed.insert(
                term.address.clone(),
                Observation {
                    clock,
                    created_ms: term.created_ms,
                    receipt: next,
                    epoch_completions,
                    epoch_token,
                },
            );
        }
        for term in &terms {
            if let Some(activity) = &term.activity {
                self.endpoint_epochs.insert(
                    endpoint_of(&term.address).to_string(),
                    (activity.core_id.clone(), activity.epoch.clone()),
                );
            }
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

#[cfg(test)]
#[path = "epoch_tests.rs"]
mod epoch_tests;

#[cfg(test)]
#[path = "current_reporter_tests.rs"]
mod current_reporter_tests;

// Derived in part from herdr src/client/shell/endpoints.rs (Apache-2.0).
//! Registry clocks constrain order; incomparable domains merge by local receipt.
//! A topological merge avoids a non-transitive pairwise ordinal/receipt comparator.
use hc_proto::terminal::Terminal;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Clock {
    domain: (String, String, u64),
    ordinal: Option<u64>,
}
impl Clock {
    pub(super) fn of(term: &Terminal) -> Option<Self> {
        let activity = term.activity.as_ref()?;
        let ordinal = activity
            .records
            .iter()
            .filter(|(id, _)| id.is_empty() || term.records.contains_key(*id))
            .flat_map(|(_, c)| [c.state_ordinal, c.completion_ordinal])
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

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Domain {
    Known((String, String, u64)),
    Unknown(String),
}

struct Observation {
    clock: Option<Clock>,
    receipt: u64,
}
#[derive(Default)]
pub(super) struct ActivityOrder {
    receipt: u64,
    observed: HashMap<String, Observation>,
}
impl ActivityOrder {
    pub(super) fn ranks(
        &mut self,
        mut clocks: Vec<(String, Option<Clock>)>,
    ) -> HashMap<String, u64> {
        let present = clocks.iter().map(|(id, _)| id).collect::<HashSet<_>>();
        self.observed.retain(|id, _| present.contains(id));
        // Same-publish ties are deterministic; ordinals are inspected only
        // after the full origin/epoch/scope domain, never across registries.
        clocks.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        for (id, clock) in &clocks {
            if self.observed.get(id).is_none_or(|old| old.clock != *clock) {
                // A lifetime reset baselines existing facts, not fresh activity.
                let previous = self.observed.get(id);
                let reset = previous.is_some_and(|old| {
                    old.clock.as_ref().map(|c| &c.domain) != clock.as_ref().map(|c| &c.domain)
                });
                let receipt = if reset {
                    previous.expect("existing observation").receipt
                } else {
                    self.receipt = self.receipt.saturating_add(1);
                    self.receipt
                };
                self.observed.insert(
                    id.clone(),
                    Observation {
                        clock: clock.clone(),
                        receipt,
                    },
                );
            }
        }
        // Known clocks form ordered domain queues. Unknown history imposes
        // no clock constraint: each such pane has an independent receipt slot.
        let mut groups = BTreeMap::<Domain, Vec<(u64, String)>>::new();
        for (id, clock) in clocks {
            let (key, ordinal) = match clock {
                Some(Clock {
                    domain,
                    ordinal: Some(n),
                }) => (Domain::Known(domain), n),
                _ => (Domain::Unknown(id.clone()), 0),
            };
            groups.entry(key).or_default().push((ordinal, id));
        }
        let mut queues = groups
            .into_values()
            .map(|mut group| {
                group.sort();
                group.into_iter().map(|(_, id)| id).collect::<VecDeque<_>>()
            })
            .collect::<Vec<_>>();
        let mut ranks = HashMap::new();
        while let Some(index) = queues
            .iter()
            .enumerate()
            .filter_map(|(index, queue)| {
                let id = queue.front()?;
                Some((index, self.observed[id].receipt, id))
            })
            .min_by(|a, b| (a.1, a.2).cmp(&(b.1, b.2)))
            .map(|v| v.0)
        {
            let id = queues[index].pop_front().expect("nonempty queue");
            ranks.insert(id, ranks.len() as u64 + 1);
        }
        ranks
    }
}

#[cfg(test)]
#[path = "activity_order_tests.rs"]
mod tests;

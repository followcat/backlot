//! The per-machine `backlot studio list --json` cache (W1).
//!
//! A machine whose stage is down (or which published no document) still
//! has studios in its ledger. The sidebar's machines tree asks each such
//! machine for the list through the same `backlot … --json` runner the
//! New Studio dialog uses, and draws the answer dim. The list refreshes
//! when the machine's stage state changes or every [`REFRESH_EVERY`] at
//! most — never on every publish.
//!
//! Pure state: no I/O, no clock; the caller passes `now`.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// How long a fetched list stays fresh while nothing else changes.
pub(crate) const REFRESH_EVERY: Duration = Duration::from_secs(30);

/// One studio the ledger lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LedgerStudio {
    /// The studio id — the name the ledger directory carries.
    pub(crate) name: String,
    pub(crate) state: LedgerState,
}

/// The ledger's own studio state word, degraded instead of refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LedgerState {
    Live,
    Finished,
    /// A ledger that will not load — listed, claiming nothing.
    Failed,
}

/// One machine to judge.
pub(crate) struct Machine {
    /// `None` is this core.
    pub(crate) machine: Option<String>,
    pub(crate) stage_running: bool,
    /// This machine's parsed `backlot` document answered.
    pub(crate) has_summary: bool,
}

#[derive(Debug, Default)]
struct Entry {
    studios: Vec<LedgerStudio>,
    fetched: Option<Instant>,
    /// The stage state last observed — a change invalidates the list.
    last_stage: Option<bool>,
    inflight: bool,
}

/// The cache, keyed by machine (`None` is this core).
#[derive(Debug, Default)]
pub(crate) struct StudioIndex {
    entries: BTreeMap<Option<String>, Entry>,
}

impl StudioIndex {
    /// The last fetched studios for `machine` — empty before the first
    /// answer.
    pub(crate) fn studios(&self, machine: Option<&str>) -> &[LedgerStudio] {
        self.entries
            .get(&machine.map(str::to_owned))
            .map(|entry| entry.studios.as_slice())
            .unwrap_or(&[])
    }

    /// A fetch for `machine` is in flight.
    #[cfg(test)]
    pub(crate) fn inflight(&self, machine: Option<&str>) -> bool {
        self.entries
            .get(&machine.map(str::to_owned))
            .is_some_and(|entry| entry.inflight)
    }

    /// The machines that want a fetch now: whose stage is down or which
    /// published no document, and whose cached list is absent, older
    /// than [`REFRESH_EVERY`], or was fetched under another stage state.
    /// Every returned machine is marked in flight.
    pub(crate) fn want(&mut self, now: Instant, machines: &[Machine]) -> Vec<Option<String>> {
        let mut out = Vec::new();
        for machine in machines {
            let entry = self.entries.entry(machine.machine.clone()).or_default();
            if entry.last_stage != Some(machine.stage_running) {
                // The stage changed since the answer: it is stale.
                entry.last_stage = Some(machine.stage_running);
                entry.fetched = None;
            }
            if machine.stage_running && machine.has_summary {
                continue;
            }
            if entry.inflight {
                continue;
            }
            let stale = entry
                .fetched
                .is_none_or(|at| now.saturating_duration_since(at) >= REFRESH_EVERY);
            if stale {
                entry.inflight = true;
                out.push(machine.machine.clone());
            }
        }
        out
    }

    /// A fetch for `machine` landed: parse the `--json` document and
    /// keep it. A failure keeps the last list and records the reason.
    pub(crate) fn note_reply(
        &mut self,
        machine: Option<String>,
        stage_running: bool,
        now: Instant,
        out: Result<&str, &str>,
    ) {
        let entry = self.entries.entry(machine).or_default();
        entry.inflight = false;
        entry.fetched = Some(now);
        entry.last_stage = Some(stage_running);
        // A failed read (or a malformed document) keeps the last list:
        // the tree shows the studios it already knows rather than an
        // empty machine.
        if let Ok(text) = out
            && let Ok(studios) = parse(text)
        {
            entry.studios = studios;
        }
    }
}

/// Parse `backlot studio list --json` — `{"pinned": bool, "studios":
/// [{"name", "state", …}]}`. Unknown fields and newer states are
/// tolerated; a row without a name is skipped.
pub(crate) fn parse(text: &str) -> Result<Vec<LedgerStudio>, String> {
    #[derive(serde::Deserialize)]
    struct Listing {
        #[serde(default)]
        studios: Vec<Row>,
    }
    #[derive(serde::Deserialize)]
    struct Row {
        name: String,
        #[serde(default)]
        state: String,
    }
    let listing: Listing = serde_json::from_str(text).map_err(|e| format!("studio list: {e}"))?;
    let mut out = Vec::new();
    for row in listing.studios {
        let name = row.name.trim();
        if name.is_empty() {
            continue;
        }
        if out.iter().any(|studio: &LedgerStudio| studio.name == name) {
            continue;
        }
        let state = match row.state.as_str() {
            "live" => LedgerState::Live,
            "finished" => LedgerState::Finished,
            _ => LedgerState::Failed,
        };
        out.push(LedgerStudio {
            name: name.to_owned(),
            state,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(name: Option<&str>, stage_running: bool, has_summary: bool) -> Machine {
        Machine {
            machine: name.map(str::to_owned),
            stage_running,
            has_summary,
        }
    }

    const HOST_B: &str = r#"{
        "pinned": true,
        "studios": [
            {"name": "beta-tools", "state": "live", "title": "Beta tools", "last_ms": 9},
            {"name": "old-shop", "state": "finished", "last_ms": 1}
        ]
    }"#;

    #[test]
    fn the_json_lists_live_and_finished_studios() {
        let studios = parse(HOST_B).unwrap();
        assert_eq!(studios.len(), 2);
        assert_eq!(studios[0].name, "beta-tools");
        assert_eq!(studios[0].state, LedgerState::Live);
        assert_eq!(studios[1].state, LedgerState::Finished);
    }

    #[test]
    fn a_studio_list_failure_is_a_line_not_a_panic() {
        assert!(parse("not json").is_err());
        let studios =
            parse(r#"{"pinned": true, "studios": [{"name": "x", "state": "warning"}]}"#).unwrap();
        assert_eq!(studios[0].state, LedgerState::Failed);
        assert!(
            parse(r#"{"studios": [{"name": "  "}]}"#)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_stage_down_machine_is_asked_at_once_then_every_thirty_seconds() {
        let mut index = StudioIndex::default();
        let now = Instant::now();
        let rows = [machine(None, false, true)];
        assert_eq!(index.want(now, &rows), vec![None]);
        assert!(index.inflight(None), "asked");
        // In flight — a second pass does not ask again.
        assert!(index.want(now, &rows).is_empty());
        index.note_reply(None, false, now, Ok(HOST_B));
        assert!(!index.inflight(None));
        assert_eq!(index.studios(None).len(), 2);
        // Fresh for thirty seconds.
        assert!(index.want(now + Duration::from_secs(29), &rows).is_empty());
        assert!(
            !index.want(now + REFRESH_EVERY, &rows).is_empty(),
            "the list ages out"
        );
    }

    #[test]
    fn a_stage_that_comes_up_stops_the_asking() {
        let mut index = StudioIndex::default();
        let now = Instant::now();
        let rows = [machine(Some("host-b"), true, true)];
        assert!(index.want(now, &rows).is_empty(), "a live stage needs none");
    }

    #[test]
    fn a_stage_change_invalidates_the_cached_list() {
        let mut index = StudioIndex::default();
        let now = Instant::now();
        let down = [machine(Some("host-b"), false, false)];
        let wanted = index.want(now, &down);
        assert_eq!(wanted, vec![Some("host-b".to_owned())]);
        index.note_reply(Some("host-b".into()), false, now, Ok(HOST_B));
        // Still down: the fresh answer stands.
        assert!(index.want(now + Duration::from_secs(5), &down).is_empty());
        // Up with a document: no ask, and the stored state is marked
        // stale for later.
        let up = [machine(Some("host-b"), true, true)];
        assert!(index.want(now + Duration::from_secs(6), &up).is_empty());
        // Down again at once: the stage changed under the answer — ask.
        assert_eq!(
            index.want(now + Duration::from_secs(7), &down),
            vec![Some("host-b".to_owned())]
        );
    }

    #[test]
    fn a_failure_keeps_the_last_list() {
        let mut index = StudioIndex::default();
        let now = Instant::now();
        index.note_reply(None, false, now, Ok(HOST_B));
        index.note_reply(
            None,
            false,
            now,
            Err("cannot run backlot: no such file\nmore"),
        );
        assert_eq!(index.studios(None).len(), 2, "the last list stands");
        // A malformed answer is equally harmless.
        index.note_reply(None, false, now, Ok("not json"));
        assert_eq!(index.studios(None).len(), 2);
    }
}

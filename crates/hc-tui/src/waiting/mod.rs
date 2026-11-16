//! Waiting: a presentation-only overlay on a shown Idle for agents
//! whose own turn is quiet while work they started still runs — a
//! background tool shell (every agent), or what an agent with an exact
//! layer-2 entry reports itself. The rules here are pure: the wire
//! probes (`machine.exec`, `file.read`) and the live-state plumbing
//! live in `app/waiting.rs`, the visual in `chrome/status.rs`. Nothing
//! here mutates core status, seen state or notifications — a Waiting
//! verdict only ever decides whether a shown `Idle` draws as `waiting`.

pub(crate) mod claude;
pub(crate) mod ps;

use crate::chrome::status::AgentStatus;
use hc_proto::terminal::Terminal;
use std::time::Instant;

/// How long after the agent's own start a tool shell may appear before
/// it counts: younger shells are startup helpers, not waiting
/// evidence.
pub(crate) const STARTUP_WINDOW_SECS: u64 = 20;

/// An exact layer-2 entry: a rule verified against the program's own
/// binary or docs, keyed by the effective record's `app`. Programs
/// without an entry get the process layer only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layer2 {
    /// Claude Code's per-process sessions file reports `shell` while
    /// background tasks (shells, monitors, in-process sub agents)
    /// outlive the turn. Read with `file.read`, only `pid` and
    /// `status` are consumed.
    ClaudeSessions,
}

/// The agent's exact entry for its current word, when one exists.
/// A `working` word has no entries yet — the table is where a
/// verified "only waiting on sub agents" message would land.
pub(crate) fn layer2_for(app: &str, word: &str) -> Option<Layer2> {
    match (app, word) {
        ("claude-code", "idle" | "done") => Some(Layer2::ClaudeSessions),
        _ => None,
    }
}

/// The terminal identity one probe answers for: the state word, the
/// foreground pid, the incarnation and the winning record. Any change
/// drops the verdict — stale evidence never overlays a new state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fingerprint {
    /// The foreground pid the process layer walks descendants of.
    pub pid: i32,
    /// The terminal incarnation — a rerun is a different agent run.
    pub created_ms: u64,
    /// The effective record's name (`None` = the root record).
    pub record: Option<String>,
    /// The effective record's raw state word.
    pub word: String,
}

impl Fingerprint {
    /// The probe identity of `term`, `None` when it cannot be probed:
    /// not a live reporter, no foreground pid, or a word the overlay
    /// never covers (blocked, error, unknown).
    pub(crate) fn of(term: &Terminal) -> Option<Self> {
        if !term.is_live_reporter() {
            return None;
        }
        let pid = term.foreground.as_ref()?.pid;
        let effective = crate::sidebar::effective(term)?;
        let word = match &effective.word {
            crate::sidebar::status::Word::Idle => "idle",
            crate::sidebar::status::Word::Done => "done",
            crate::sidebar::status::Word::Working { .. } => "working",
            _ => return None,
        };
        Some(Fingerprint {
            pid,
            created_ms: term.created_ms,
            record: effective.record.clone(),
            word: word.to_owned(),
        })
    }
}

/// What one probe cycle asks of a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProbePlan {
    /// The terminal's address — results route back on it.
    pub address: String,
    /// `machine.exec`/`file.read`'s `on`: the machine the terminal
    /// runs on; `None` is the local machine.
    pub machine: Option<String>,
    /// The identity the verdict applies to.
    pub fingerprint: Fingerprint,
    /// Layer 1: walk `fingerprint.pid`'s descendants for tool shells.
    pub process_tree: bool,
    /// Layer 2: the agent's exact entry, when it has one.
    pub layer2: Option<Layer2>,
}

/// The probes one cycle runs for `term`, `None` when nothing applies.
pub(crate) fn probe_plan(term: &Terminal) -> Option<ProbePlan> {
    let fingerprint = Fingerprint::of(term)?;
    let status = crate::project::effective_status(term)?;
    let (process_tree, layer2) = match fingerprint.word.as_str() {
        "idle" | "done" => (true, layer2_for(&status.app, &fingerprint.word)),
        "working" => (false, layer2_for(&status.app, &fingerprint.word)),
        _ => return None,
    };
    (process_tree || layer2.is_some()).then(|| ProbePlan {
        address: term.address.clone(),
        machine: crate::project::machine_of(&term.address).map(str::to_owned),
        fingerprint,
        process_tree,
        layer2,
    })
}

/// A probe's verdict for one terminal.
#[derive(Debug, Clone)]
pub(crate) struct Verdict {
    /// The identity it was probed against — a stale verdict never
    /// applies to a changed state, pid, incarnation or record.
    pub fingerprint: Fingerprint,
    /// Background work was found: the same tool shell showed in two
    /// consecutive listings, or a layer-2 entry reported it.
    pub waiting: bool,
    /// The last listing sighted a tool shell once — not evidence yet,
    /// but the next listing decides, so the fast cadence applies.
    pub candidate: bool,
    /// When the cycle was queued — the re-probe cadence counts from it.
    pub probed_at: Instant,
}

/// Live verdicts by terminal address.
pub(crate) type Verdicts = std::collections::HashMap<String, Verdict>;

/// The presentation overlay: Waiting stands in for a shown `Idle`
/// only — an unseen `Done` keeps its completion, every other shown
/// status passes through.
pub(crate) fn overlay(status: AgentStatus, waiting: bool) -> AgentStatus {
    match (status, waiting) {
        (AgentStatus::Idle, true) => AgentStatus::Waiting,
        (status, _) => status,
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "ps_tests.rs"]
mod ps_tests;

#[cfg(test)]
#[path = "claude_tests.rs"]
mod claude_tests;

//! The `backlot` summary document: one compact view of every studio
//! for the panel, derived purely from reduced ledger state. It holds
//! no credentials, environment values, prompt text or session IDs —
//! the ledger stays the source of truth.
//!
//! `summary()` builds the view; the stage writes it under the CAS
//! version, only on change and at most once every two seconds. Sizes
//! are capped so the document stays well under its 64 KiB target.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::attention::AttentionClass;
use crate::event::{Priority, StudioMode, UsageTotals};
use crate::state::{StudioState, TaskStatus};
use crate::summary_build::*;

pub use crate::summary_build::role_name;

// Hollywood's section types live beside this file; the
// `summary::` path keeps them all.
pub use crate::hollywood_summary::{
    CardSummary, Exchange, HEADLINE_CHARS, HollywoodSummary, MAX_EXCHANGE, MAX_OPEN_CARDS,
    hollywood_section, hollywood_section_with_activity, hollywood_section_with_work,
};
pub use crate::studio_exchange::{
    MAX_STUDIO_EXCHANGE, StudioExchange, studio_exchange, studio_exchange_files,
};

/// The `schema` value this generation writes.
/// The `schema` value this generation writes.
pub const SUMMARY_SCHEMA: &str = "backlot/1";

/// A live role's screen-classified work state (R17): the stage's
/// watch pass samples it into the summary — the ledger never sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Work {
    Working,
    Waiting,
    Blocked,
}

/// Most studios shown; older finished ones drop off first.
/// Most studios shown; older finished ones drop off first.
pub const MAX_STUDIOS: usize = 12;
/// Per-string cap so nothing can bloat the document.
/// Per-string cap so nothing can bloat the document.
const CAP: usize = 96;
/// Serialized size the builder aims for (the design target is 64 KiB;
/// the store's hard limit is 1 MiB).
/// Serialized size the builder aims for (the design target is 64 KiB;
/// the store's hard limit is 1 MiB).
const TARGET_BYTES: usize = 60_000;

/// One studio's roles + counters as the panel needs them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StudioSummary {
    pub id: String,
    pub title: String,
    /// `live` while work can still come; `finished` when every task is
    /// terminal and no role is live.
    pub state: String,
    pub requirement_version: u32,
    /// Current requirement's studio-relative path (`req/v<n>.md`),
    /// absent while `requirement_version == 0` or on older stage docs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub req_file: Option<String>,
    pub mode: StudioMode,
    /// Delivered tasks still awaiting a review.
    pub open_reviews: u32,
    /// Open attention records — what is waiting on a person.
    pub waiting: u32,
    /// The stage's last error for this studio — an unreadable ledger
    /// or a failed pass — while it stays broken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The first step the ledger still owes, as `"label — owner"`;
    /// `None` while nothing is owed (S9a).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    /// The studio's repository — a studio shell's fallback working
    /// directory (U2). Documents an older stage wrote have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// Every task the ledger knows is terminal and `studio.finished`
    /// was never written — the work is done, the finish is still the
    /// user's. Documents an older stage wrote read as `false`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub ready_to_finish: bool,
    /// Tasks in `accepted` or `partially accepted` — the ready row's
    /// `<done>/<total>` (S1). Documents an older stage wrote read 0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tasks_done: u32,
    /// Every task the ledger recorded.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tasks_total: u32,
    /// Token/cost totals the studio's deliveries reported (P4a) —
    /// factual sums only, never a score (D8). Absent while zero so
    /// documents for usage-less studios stay byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageTotals>,
    /// The compact per-task projection the dock's task rows and the
    /// task sheet read (S2) — enough to draw every task state without
    /// a records call. Documents an older stage wrote have none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<TaskSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<RoleSummary>,
    /// The newest [`MAX_STUDIO_EXCHANGE`] user-facing messages and
    /// `to: user` reports, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exchange: Vec<StudioExchange>,
    /// True while an undelivered, non-dropped user `tell` sits in
    /// this studio's outbox behind the send gate.
    #[serde(default, skip_serializing_if = "is_false")]
    pub pending_tell: bool,
}

/// One task's dock projection — the row the panel draws even when no
/// live actor carries it (A4.2). No event payloads, no file text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskSummary {
    pub id: String,
    pub title: String,
    pub status: TaskStatus,
    pub priority: Priority,
    /// The requirement version the task was dispatched under (pin).
    pub requirement_version: u32,
    /// Unfinished and pinned behind the studio's current version.
    #[serde(default, skip_serializing_if = "is_false")]
    pub behind: bool,
    pub rework_rounds: u32,
    pub rework_cap: u32,
    /// The current cast's role key (`actor-t1-c1`) and its candidate
    /// display name — `None` while the task is uncast.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_name: Option<String>,
    /// Dependencies not yet terminal — what the `dep` row names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waits_for: Vec<String>,
    /// The latest delivery number and its actor-reported checks as
    /// `name:pass|fail` — the drawer's `d1 units:pass`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_delivery: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_checks: Option<String>,
    /// The latest review's verdict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_verdict: Option<crate::event::Verdict>,
}

/// One recorded role: live, lost or closed. Closed roles stay listed
/// while their studio is live — with no `doing` text (R19: closed,
/// not deleted).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleSummary {
    /// The ledger role key (`producer`, `actor-t1-c1`).
    pub role: String,
    /// The candidate's name for actors; the role name otherwise.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// `live`, `lost` or `closed`.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doing: Option<String>,
    /// `working`, `waiting` or `blocked` — only while live and the
    /// watch pass has classified the screen (R17).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<Work>,
    /// The error a live role's last hook signal carried when it was
    /// a `StopFailure` — the panel shows `last turn failed: <error>`
    /// until the role's next signal (S9f).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<String>,
    /// The workarea this person runs in — an actor's worktree, the
    /// director's integration worktree, the repository otherwise. A
    /// studio shell opens where the focused person works (U2).
    /// Documents an older stage wrote have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workdir: Option<String>,
    /// The task an actor works on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// The task's display priority — actors only (D11).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    /// A closed person `role.reopen` could take: it kept a session
    /// id and is not already held. Session ids never reach the
    /// document — this is the flag the panel reads instead.
    #[serde(default, skip_serializing_if = "is_false")]
    pub resumable: bool,
    #[serde(default)]
    pub since_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention: Option<String>,
    /// What the open `attention` means to the person: `needs_you`,
    /// `stuck` or `info`. Stage bookkeeping never changes a person's
    /// shown state. Documents an older stage wrote have none — the
    /// client then sorts the reason itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_class: Option<AttentionClass>,
    /// The role's newest `tell user` the user has not answered with a
    /// `tell` back to that role — shown as `asks you · <question>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asks: Option<String>,
    /// The open `attention` is a `permission prompt` only the user may
    /// answer — a flagged screen, or a prompt on a non-actor role
    /// (D9). Documents an older stage wrote read as `false`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub attention_user: bool,
    /// The first open attention record's seq — the `#<seq>` the Call
    /// Sheet shows and `attention clear <seq>` takes (S1). `None`
    /// while the role has no open attention.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_seq: Option<u64>,
    /// The first open attention's detail minus the
    /// `"; [needs the user; ]screen saved to attention/<seq>.txt"`
    /// tail — the prompt row's second line (S1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_detail: Option<String>,
    /// When the stage last answered a dialog on this role's screen
    /// (`stage.answered`) — the drawer's `trust answered by stage`
    /// line. Not an attention: nothing here feeds `!N` or digests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_answer_ms: Option<u64>,
    /// Whether the runner can answer its own prompt — at least one
    /// approval key exists. `false` means `y`/`d` would record a
    /// decision and send nothing, so the client offers `answer in its
    /// terminal (Enter)` instead. Absent on documents an older stage
    /// wrote, which keeps today's `y`/`d` behaviour.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub can_answer_prompt: bool,
    /// When the stage last restarted this role in place
    /// (`role.restart`) — the person row and Hollywood read
    /// `restarted`. Documents an older stage wrote have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restarted_ms: Option<u64>,
}

pub(crate) fn default_true() -> bool {
    true
}

pub(crate) fn is_true(b: &bool) -> bool {
    *b
}

/// The whole document body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub schema: String,
    pub updated_ms: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub studios: Vec<StudioSummary>,
    /// Hollywood's section — absent before it is configured; older
    /// documents without it still parse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hollywood: Option<HollywoodSummary>,
    /// The publishing stage's `backlot` version, as `backlot
    /// --version` prints it (`0.2.4`, no `v`) — clients read a
    /// machine's backlot version straight from its document without
    /// probing (U6c). Documents an older stage wrote read as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backlot_version: Option<String>,
}

impl Summary {
    /// Everything but the stamp — the writer compares on this.
    /// `backlot_version` does compare: an upgraded stage publishes
    /// its new version exactly once.
    pub fn same_as(&self, other: &Summary) -> bool {
        let mut a = self.clone();
        let mut b = other.clone();
        a.updated_ms = 0;
        b.updated_ms = 0;
        a == b
    }
}

pub(crate) fn cap(s: &str) -> String {
    s.chars().take(CAP).collect()
}

/// The status text without its ticking parts: a trailing run of
/// elapsed-time, percent or count tokens, plus any spinner-frame
/// token. A live runner appends these every tick while the meaning
/// stays the same — the document carries the words, not the clock,
/// so the publish throttle's `same_as` stays true between them. A
/// message reduced to nothing reads as no report.
pub(crate) fn steady(msg: &str) -> String {
    let mut toks: Vec<&str> = msg.split_whitespace().collect();
    while toks.last().is_some_and(|t| volatile_token(t)) {
        toks.pop();
    }
    toks.into_iter()
        .map(|t| t.trim_matches(|c| SPIN.contains(&c)))
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Characters that only ever wrap or separate tokens.
const EDGE: &[char] = &[
    '(', ')', '[', ']', '{', '}', '<', '>', '·', '•', '—', '–', '~', '|',
];

/// Spinner glyphs — braille frames, the star frames the claude family
/// uses, and the ASCII `| / \` trio.
const SPIN: &[char] = &[
    '⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏', '⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷', '◐',
    '◓', '◑', '◒', '✶', '✻', '✽', '✼', '✳', '✱', '✲', '❋', '❊', '❉', '✢', '✣', '✤', '✥', '|', '/',
    '\\',
];

/// A whole token of spinner frames.
fn is_spin(t: &str) -> bool {
    !t.is_empty() && t.chars().all(|c| SPIN.contains(&c))
}

/// A token the tail strip drops: a bare separator, a spinner frame,
/// or a ticking value (duration, clock, percent, count).
fn volatile_token(t: &str) -> bool {
    let t = t.trim_matches(|c| EDGE.contains(&c));
    t.is_empty() || is_spin(t) || is_tick(t)
}

/// A ticking token: `47%`, `×7`/`7×`, a duration (`500ms`, `12s`,
/// `3m`, `1.5h`, `2d`, `2m30s`) or a clock (`12:34`, `1:23:45`).
fn is_tick(t: &str) -> bool {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit() || b == b'.');
    if let Some(n) = t.strip_suffix('%') {
        return digits(n);
    }
    if let Some(n) = t.strip_prefix('×').or_else(|| t.strip_suffix('×')) {
        return digits(n);
    }
    is_duration(t)
}

fn is_duration(t: &str) -> bool {
    if t.contains(':') {
        let parts: Vec<&str> = t.split(':').collect();
        return (2..=3).contains(&parts.len())
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    }
    let mut rest = t;
    let mut pairs = 0;
    'outer: while !rest.is_empty() {
        for unit in ["ms", "s", "m", "h", "d"] {
            if let Some(head) = rest.strip_suffix(unit) {
                // The unit must sit behind at least one digit or dot.
                let trimmed = head.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
                if trimmed.len() < head.len() {
                    rest = trimmed;
                    pairs += 1;
                    continue 'outer;
                }
            }
        }
        return false;
    }
    pairs > 0
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// Build the document body from every studio's reduced state.
///
/// `studios` comes most-recently-changed first (the stage orders by
/// ledger mtime): live studios always keep their slot, and finished
/// ones fill what remains — so old finished studios drop off.
/// `live` maps a live terminal's address to what it reports doing.
/// `work` maps `(studio, role)` to the watch pass's latest work
/// classification and `failed` `(studio, role)` to the StopFailure
/// error its last hook signal carried — the summary stays pure; the
/// stage owns both maps. `errors` maps a studio to the stage's last
/// error for it — the row carries it, and a studio whose state
/// never loaded still gets a stub row so the panel can show why.
pub fn summary(
    studios: &[(String, StudioState)],
    live: &BTreeMap<String, String>,
    work: &BTreeMap<(String, String), Work>,
    failed: &BTreeMap<(String, String), String>,
    errors: &BTreeMap<String, String>,
    updated_ms: u64,
) -> Summary {
    summary_with_exchange(
        studios,
        live,
        work,
        failed,
        errors,
        &BTreeMap::new(),
        updated_ms,
    )
}

/// [`summary`] with stage-read `(studio, seq) -> snippet` lines so
/// `StudioSummary.exchange` participates in [`TARGET_BYTES`] sizing
/// while `backlot-model` stays pure.
pub fn summary_with_exchange(
    studios: &[(String, StudioState)],
    live: &BTreeMap<String, String>,
    work: &BTreeMap<(String, String), Work>,
    failed: &BTreeMap<(String, String), String>,
    errors: &BTreeMap<String, String>,
    snippets: &BTreeMap<(String, u64), String>,
    updated_ms: u64,
) -> Summary {
    let mut live_studios = Vec::new();
    let mut finished_studios = Vec::new();
    for (name, state) in studios {
        let error = errors.get(name).map(|e| cap(e));
        let Some(cfg) = &state.studio else {
            // No studio event: normally not a studio at all — but a
            // broken ledger still gets a stub so its error shows.
            if let Some(error) = error {
                live_studios.push(StudioSummary {
                    id: cap(name),
                    title: String::new(),
                    state: "live".into(),
                    requirement_version: 0,
                    req_file: None,
                    mode: StudioMode::default(),
                    open_reviews: 0,
                    waiting: 0,
                    error: Some(error),
                    next: None,
                    repo: None,
                    ready_to_finish: false,
                    tasks_done: 0,
                    tasks_total: 0,
                    usage: None,
                    tasks: Vec::new(),
                    roles: Vec::new(),
                    exchange: Vec::new(),
                    pending_tell: false,
                });
            }
            continue;
        };
        let done = finished(state);
        let mut roles: Vec<RoleSummary> = role_keys(state)
            .into_iter()
            .filter_map(|role| role_summary(name, state, role, live, work, failed, snippets))
            .collect();
        // Runner limits (S9e): a held wanted role shows `waiting for
        // a <runner> slot` — overriding a recorded closed/lost row,
        // or as a synthetic row for a role never started.
        let held = crate::limits::held(state);
        for rs in roles.iter_mut() {
            if let Some(runner) = held.get(&rs.role) {
                rs.state = "waiting".into();
                rs.doing = Some(cap(&format!("waiting for a {runner} slot")));
                rs.work = None;
                rs.failed = None;
            }
        }
        for w in crate::wanted::wanted_roles(state) {
            let Some(runner) = held.get(&w.role) else {
                continue;
            };
            if roles.iter().any(|r| r.role == w.role) {
                continue;
            }
            roles.push(RoleSummary {
                role: cap(&w.role),
                name: role_name(state, &w.role),
                terminal: None,
                runner: Some(cap(&w.actor.runner)),
                model: Some(cap(&w.actor.model)),
                effort: w
                    .actor
                    .effort
                    .clone()
                    .filter(|e| !e.is_empty())
                    .map(|e| cap(&e)),
                state: "waiting".into(),
                doing: Some(cap(&format!("waiting for a {runner} slot"))),
                work: None,
                failed: None,
                workdir: None,
                task: w.task.as_ref().map(|t| cap(t)),
                priority: w
                    .task
                    .as_deref()
                    .and_then(|t| state.tasks.get(t))
                    .map(|t| t.priority),
                resumable: false,
                since_ms: 0,
                attention: None,
                attention_class: None,
                asks: None,
                attention_user: false,
                attention_seq: None,
                attention_detail: None,
                stage_answer_ms: None,
                can_answer_prompt: crate::caps::can_answer_prompt(&w.actor.runner),
                restarted_ms: None,
            });
        }
        let studio_snippets: BTreeMap<u64, String> = snippets
            .iter()
            .filter_map(|((s, seq), text)| (s == name).then_some((*seq, text.clone())))
            .collect();
        let entry = StudioSummary {
            id: cap(name),
            title: cap(&cfg.title),
            state: if done { "finished" } else { "live" }.into(),
            requirement_version: state.requirement_version,
            req_file: state
                .requirements
                .iter()
                .rfind(|r| r.version == state.requirement_version)
                .or_else(|| state.requirements.last())
                .map(|r| cap(&r.file)),
            mode: cfg.mode,
            open_reviews: state
                .tasks
                .values()
                .filter(|t| t.status == TaskStatus::Delivered)
                .count() as u32,
            waiting: state.open_attention().count() as u32,
            error,
            next: crate::steps::next_steps(state)
                .first()
                .map(|s| cap(&format!("{} — {}", s.label, s.owner))),
            repo: Some(cap(&cfg.repo)),
            ready_to_finish: !state.tasks.is_empty()
                && state.tasks.values().all(|t| t.status.is_terminal())
                && state.finished.is_none(),
            tasks_done: state
                .tasks
                .values()
                .filter(|t| {
                    matches!(
                        t.status,
                        TaskStatus::Accepted | TaskStatus::PartiallyAccepted
                    )
                })
                .count() as u32,
            tasks_total: state.tasks.len() as u32,
            usage: (!state.usage.is_empty()).then_some(state.usage),
            tasks: state
                .tasks
                .values()
                .map(|t| task_summary(state, t))
                .collect(),
            roles,
            exchange: crate::studio_exchange::studio_exchange(state, &studio_snippets),
            pending_tell: state
                .messages
                .iter()
                .any(|m| m.from == "user" && m.to != "user" && !m.delivered && !m.dropped),
        };
        if done {
            finished_studios.push(entry);
        } else {
            live_studios.push(entry);
        }
    }
    let mut out: Vec<StudioSummary> = live_studios;
    for s in finished_studios {
        if out.len() >= MAX_STUDIOS {
            break;
        }
        out.push(s);
    }
    // Shrink toward the target by dropping finished studios — they
    // sit at the tail. A live studio is never dropped: if the body
    // is still over, it is written anyway (64 KiB is the target,
    // 1 MiB the hard limit).
    while out.last().is_some_and(|s| s.state == "finished")
        && serde_json::to_vec(&out)
            .map(|b| b.len() > TARGET_BYTES)
            .unwrap_or(false)
    {
        out.pop();
    }
    Summary {
        schema: SUMMARY_SCHEMA.into(),
        updated_ms,
        studios: out,
        hollywood: None,
        backlot_version: None,
    }
}

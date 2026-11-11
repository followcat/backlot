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

use crate::event::{Priority, StudioMode};
use crate::state::{RoleLifecycle, StudioState, TaskStatus, actor_task};

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
pub const MAX_STUDIOS: usize = 12;
/// Per-string cap so nothing can bloat the document.
const CAP: usize = 96;
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<RoleSummary>,
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
}

/// The whole document body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub schema: String,
    pub updated_ms: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub studios: Vec<StudioSummary>,
}

impl Summary {
    /// Everything but the stamp — the writer compares on this.
    pub fn same_as(&self, other: &Summary) -> bool {
        let mut a = self.clone();
        let mut b = other.clone();
        a.updated_ms = 0;
        b.updated_ms = 0;
        a == b
    }
}

fn cap(s: &str) -> String {
    s.chars().take(CAP).collect()
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A studio is finished when every task it knows is terminal and no
/// recorded role is still live.
fn finished(state: &StudioState) -> bool {
    !state.tasks.is_empty()
        && state.tasks.values().all(|t| t.status.is_terminal())
        && state
            .roles
            .values()
            .all(|r| r.lifecycle() != RoleLifecycle::Live)
}

/// Role keys in panel order: the fixed roles first, then the actors —
/// open people (live or lost) before closed, each group sorted by
/// the task's priority high-first and then by role key. Every
/// recorded role is listed — a closed person stays while the studio
/// is live (R19), so there is no count cap.
fn role_keys(state: &StudioState) -> Vec<&str> {
    const FIXED: [&str; 3] = ["producer", "director", "writer"];
    let mut actors: Vec<&str> = state
        .roles
        .keys()
        .map(String::as_str)
        .filter(|k| !FIXED.contains(k))
        .collect();
    actors.sort_by(|a, b| {
        let closed = |k: &str| state.roles[k].lifecycle() == RoleLifecycle::Closed;
        closed(a)
            .cmp(&closed(b))
            .then(role_priority(state, b).cmp(&role_priority(state, a)))
            .then(a.cmp(b))
    });
    FIXED
        .into_iter()
        .filter(|k| state.roles.contains_key(*k))
        .chain(actors)
        .collect()
}

/// An actor's task priority for ordering — `normal` when the key
/// names no task.
fn role_priority(state: &StudioState, role: &str) -> Priority {
    actor_task(role)
        .and_then(|t| state.tasks.get(t))
        .map(|t| t.priority)
        .unwrap_or_default()
}

/// The candidate name for `actor-<task>-c<n>` keys, else the key.
fn role_name(state: &StudioState, role: &str) -> String {
    let base = || cap(role);
    let Some(rest) = role.strip_prefix("actor-") else {
        return base();
    };
    let Some((task, n)) = rest.rsplit_once("-c") else {
        return base();
    };
    let Ok(n) = n.parse::<usize>() else {
        return base();
    };
    state
        .tasks
        .get(task)
        .and_then(|t| t.casts.get(n.saturating_sub(1)))
        .map(|c| cap(&c.candidate))
        .unwrap_or_else(base)
}

/// The first open attention on this role, if any. A `permission
/// prompt` the director may not answer — a flagged `BACKLOT_*`
/// screen, or any prompt on a role that is not an actor — is marked
/// as the user's (S9b).
fn role_attention(state: &StudioState, role: &str) -> Option<String> {
    state.open_attention().find(|a| a.role == role).map(|a| {
        if a.reason == crate::event::PERMISSION_PROMPT
            && (a.needs_user || crate::state::actor_task(&a.role).is_none())
        {
            cap(&format!("{} — needs the user", a.reason))
        } else {
            cap(&a.reason)
        }
    })
}

/// One role's summary. `live` maps a terminal address to what the
/// program there reports doing; `work` the watch pass's per-role
/// screen classification and `failed` a role's StopFailure error,
/// both keyed `(studio, role)`.
fn role_summary(
    studio: &str,
    state: &StudioState,
    role: &str,
    live: &BTreeMap<String, String>,
    work: &BTreeMap<(String, String), Work>,
    failed: &BTreeMap<(String, String), String>,
) -> Option<RoleSummary> {
    let r = state.roles.get(role)?;
    let lifecycle = r.lifecycle();
    let terminal = r.terminal.clone();
    // `doing`: what a live terminal reports; actors fall back to their
    // task. Lost and closed roles have none.
    let doing = match lifecycle {
        RoleLifecycle::Live => terminal
            .as_ref()
            .and_then(|t| live.get(t))
            .map(|d| cap(d))
            .or_else(|| {
                actor_task(role).map(|t| {
                    if state
                        .tasks
                        .get(t)
                        .is_some_and(|task| task.is_behind(state.requirement_version))
                    {
                        format!("task {} · behind v{}", cap(t), state.requirement_version)
                    } else {
                        format!("task {}", cap(t))
                    }
                })
            }),
        _ => None,
    };
    // Work is a live screen's read — a stale sample for a role that
    // stopped being live is dropped here, never shown.
    let work = (lifecycle == RoleLifecycle::Live)
        .then(|| work.get(&(studio.to_string(), role.to_string())))
        .flatten()
        .copied();
    // Like `work`: a live read only, dropped for a lost or closed role.
    let failed = (lifecycle == RoleLifecycle::Live)
        .then(|| failed.get(&(studio.to_string(), role.to_string())))
        .flatten()
        .map(|e| cap(e));
    let task = actor_task(role).map(String::from);
    let priority = task
        .as_deref()
        .and_then(|t| state.tasks.get(t))
        .map(|t| t.priority);
    let actor = r.actor.as_ref();
    Some(RoleSummary {
        role: cap(role),
        name: role_name(state, role),
        terminal: terminal.map(|t| cap(&t)),
        runner: actor.map(|a| cap(&a.runner)),
        model: actor.map(|a| cap(&a.model)),
        effort: actor.and_then(|a| (!a.effort.is_empty()).then(|| cap(&a.effort))),
        state: match lifecycle {
            RoleLifecycle::Live => "live",
            RoleLifecycle::Lost => "lost",
            RoleLifecycle::Closed => "closed",
        }
        .into(),
        doing,
        work,
        failed,
        task: task.map(|t| cap(&t)),
        priority,
        resumable: lifecycle == RoleLifecycle::Closed && r.session_id.is_some() && !r.held,
        since_ms: r.since_ms,
        attention: role_attention(state, role),
    })
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
                    mode: StudioMode::default(),
                    open_reviews: 0,
                    waiting: 0,
                    error: Some(error),
                    next: None,
                    roles: Vec::new(),
                });
            }
            continue;
        };
        let done = finished(state);
        let mut roles: Vec<RoleSummary> = role_keys(state)
            .into_iter()
            .filter_map(|role| role_summary(name, state, role, live, work, failed))
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
                effort: (!w.actor.effort.is_empty()).then(|| cap(&w.actor.effort)),
                state: "waiting".into(),
                doing: Some(cap(&format!("waiting for a {runner} slot"))),
                work: None,
                failed: None,
                task: w.task.as_ref().map(|t| cap(t)),
                priority: w
                    .task
                    .as_deref()
                    .and_then(|t| state.tasks.get(t))
                    .map(|t| t.priority),
                resumable: false,
                since_ms: 0,
                attention: None,
            });
        }
        let entry = StudioSummary {
            id: cap(name),
            title: cap(&cfg.title),
            state: if done { "finished" } else { "live" }.into(),
            requirement_version: state.requirement_version,
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
            roles,
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
    }
}

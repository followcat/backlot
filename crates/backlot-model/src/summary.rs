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

/// The first open attention on this role, if any.
fn role_attention(state: &StudioState, role: &str) -> Option<String> {
    state
        .open_attention()
        .find(|a| a.role == role)
        .map(|a| cap(&a.reason))
}

/// One role's summary. `live` maps a terminal address to what the
/// program there reports doing; `work` the watch pass's per-role
/// screen classification, keyed `(studio, role)`.
fn role_summary(
    studio: &str,
    state: &StudioState,
    role: &str,
    live: &BTreeMap<String, String>,
    work: &BTreeMap<(String, String), Work>,
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
        effort: actor.map(|a| cap(&a.effort)),
        state: match lifecycle {
            RoleLifecycle::Live => "live",
            RoleLifecycle::Lost => "lost",
            RoleLifecycle::Closed => "closed",
        }
        .into(),
        doing,
        work,
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
/// `work` maps `(studio, role)` to the watch pass's latest screen
/// classification — the summary stays pure; the stage owns the map.
pub fn summary(
    studios: &[(String, StudioState)],
    live: &BTreeMap<String, String>,
    work: &BTreeMap<(String, String), Work>,
    updated_ms: u64,
) -> Summary {
    let mut live_studios = Vec::new();
    let mut finished_studios = Vec::new();
    for (name, state) in studios {
        let Some(cfg) = &state.studio else {
            continue;
        };
        let done = finished(state);
        let roles: Vec<RoleSummary> = role_keys(state)
            .into_iter()
            .filter_map(|role| role_summary(name, state, role, live, work))
            .collect();
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

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

use crate::event::StudioMode;
use crate::state::{RoleLifecycle, StudioState, TaskStatus};

/// The `schema` value this generation writes.
pub const SUMMARY_SCHEMA: &str = "backlot/1";

/// Most studios shown; older finished ones drop off first.
pub const MAX_STUDIOS: usize = 12;
/// Most roles shown per studio.
pub const MAX_ROLES: usize = 16;
/// Per-string cap so nothing can bloat the document.
const CAP: usize = 96;
/// Serialized size the builder stays under (the design target is 64 KiB).
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
/// program there reports doing.
fn role_summary(
    state: &StudioState,
    role: &str,
    live: &BTreeMap<String, String>,
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
                role.strip_prefix("actor-")
                    .and_then(|rest| rest.rsplit_once("-c").map(|(t, _)| t))
                    .map(|t| format!("task {}", cap(t)))
            }),
        _ => None,
    };
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
pub fn summary(
    studios: &[(String, StudioState)],
    live: &BTreeMap<String, String>,
    updated_ms: u64,
) -> Summary {
    let mut live_studios = Vec::new();
    let mut finished_studios = Vec::new();
    for (name, state) in studios {
        let Some(cfg) = &state.studio else {
            continue;
        };
        let done = finished(state);
        let roles: Vec<RoleSummary> = state
            .roles
            .keys()
            .filter_map(|role| role_summary(state, role, live))
            .take(MAX_ROLES)
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
    // The target size is a promise: drop trailing studios until the
    // serialized body fits.
    while serde_json::to_vec(&out)
        .map(|b| b.len() > TARGET_BYTES)
        .unwrap_or(false)
        && out.len() > 1
    {
        out.pop();
    }
    Summary {
        schema: SUMMARY_SCHEMA.into(),
        updated_ms,
        studios: out,
    }
}

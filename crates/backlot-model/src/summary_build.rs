//! The summary's derivation half: `Task`, `RoleSummary` and the
//! attention/role projections `summary()` assembles into the
//! `backlot` document. Pure functions over ledger state — no
//! credentials, prompt text or session IDs pass through.

use std::collections::BTreeMap;

use crate::event::Priority;
use crate::state::{RoleLifecycle, StudioState, Task, actor_task};
use crate::summary::{RoleSummary, TaskSummary, Work, cap};

/// A studio is finished when every task it knows is terminal and no
/// recorded role is still live.
pub(crate) fn finished(state: &StudioState) -> bool {
    !state.tasks.is_empty()
        && state.tasks.values().all(|t| t.status.is_terminal())
        && state
            .roles
            .values()
            .all(|r| r.lifecycle() != RoleLifecycle::Live)
}

/// One task's dock projection (S2): the row state, the dependency
/// waits, the current cast's key and candidate name, the latest
/// delivery's number and actor-reported checks, the latest verdict.
/// One task's dock projection (S2): the row state, the dependency
/// waits, the current cast's key and candidate name, the latest
/// delivery's number and actor-reported checks, the latest verdict.
pub(crate) fn task_summary(state: &StudioState, task: &Task) -> TaskSummary {
    let latest = task.deliveries.last();
    let checks = latest.and_then(|d| {
        (!d.checks.is_empty()).then(|| {
            cap(&d
                .checks
                .iter()
                .map(|c| format!("{}:{}", c.name, if c.passed { "pass" } else { "fail" }))
                .collect::<Vec<_>>()
                .join(" "))
        })
    });
    let (actor_role, actor_name) = match &task.cast {
        Some(cast) => (
            Some(cap(&format!("actor-{}-c{}", task.id, task.casts.len()))),
            Some(cap(&cast.candidate)),
        ),
        None => (None, None),
    };
    TaskSummary {
        id: cap(&task.id),
        title: cap(&task.title),
        status: task.status,
        priority: task.priority,
        requirement_version: task.requirement_version,
        behind: task.is_behind(state.requirement_version),
        rework_rounds: task.rework_rounds,
        rework_cap: state.rework_cap(),
        actor_role,
        actor_name,
        waits_for: task
            .dependencies
            .iter()
            .filter(|d| state.tasks.get(*d).is_some_and(|t| !t.status.is_terminal()))
            .map(|d| cap(d))
            .collect(),
        latest_delivery: latest.map(|d| d.n),
        latest_checks: checks,
        latest_verdict: task.reviews.last().map(|r| r.verdict),
    }
}

/// Role keys in panel order: the fixed roles first, then the actors —
/// open people (live or lost) before closed, each group sorted by
/// the task's priority high-first and then by role key. Every
/// recorded role is listed — a closed person stays while the studio
/// is live (R19), so there is no count cap.
/// Role keys in panel order: the fixed roles first, then the actors —
/// open people (live or lost) before closed, each group sorted by
/// the task's priority high-first and then by role key. Every
/// recorded role is listed — a closed person stays while the studio
/// is live (R19), so there is no count cap.
pub(crate) fn role_keys(state: &StudioState) -> Vec<&str> {
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
/// An actor's task priority for ordering — `normal` when the key
/// names no task.
pub(crate) fn role_priority(state: &StudioState, role: &str) -> Priority {
    actor_task(role)
        .and_then(|t| state.tasks.get(t))
        .map(|t| t.priority)
        .unwrap_or_default()
}

/// The candidate name for `actor-<task>-c<n>` keys, else the key.
/// Public: the stage names a role terminal's `herdr.tab` by it.
/// The candidate name for `actor-<task>-c<n>` keys, else the key.
/// Public: the stage names a role terminal's `herdr.tab` by it.
pub fn role_name(state: &StudioState, role: &str) -> String {
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
/// The first open attention on this role, if any. A `permission
/// prompt` the director may not answer — a flagged `BACKLOT_*`
/// screen, or any prompt on a role that is not an actor — is marked
/// as the user's (S9b).
pub(crate) fn role_attention(state: &StudioState, role: &str) -> Option<String> {
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

/// The open attention's display detail (S1): the
/// `"; [needs the user; ]screen saved to attention/<seq>.txt"` tail
/// trimmed, then the usual cap. A raise that carried no tool line at
/// all — a prompt nothing bound to — reads `unbound prompt — peek
/// screen`.
/// The open attention's display detail (S1): the
/// `"; [needs the user; ]screen saved to attention/<seq>.txt"` tail
/// trimmed, then the usual cap. A raise that carried no tool line at
/// all — a prompt nothing bound to — reads `unbound prompt — peek
/// screen`.
pub(crate) fn attention_detail(a: &crate::state::Attention) -> String {
    let tail = format!("screen saved to attention/{}.txt", a.seq);
    let body = a
        .detail
        .rsplit_once(tail.as_str())
        .map(|(head, _)| head)
        .unwrap_or(a.detail.as_str());
    let body = trim_tail(body);
    let body = body.strip_suffix("needs the user").map_or(body, trim_tail);
    if body.is_empty() {
        "unbound prompt — peek screen".to_string()
    } else {
        cap(body)
    }
}

/// `"; "`, `" — "` and whitespace off a detail's right edge.
pub(crate) fn trim_tail(s: &str) -> &str {
    s.trim_end_matches(|c: char| c == ';' || c.is_whitespace() || c == '—')
}

/// The role's open attention is the user's to answer — the same rule
/// the `needs the user` marker follows, as a flag clients can count
/// without re-reading the text (the lot pill's urgent count).
/// The role's open attention is the user's to answer — the same rule
/// the `needs the user` marker follows, as a flag clients can count
/// without re-reading the text (the lot pill's urgent count).
pub(crate) fn role_attention_user(state: &StudioState, role: &str) -> bool {
    state.open_attention().any(|a| {
        a.role == role
            && a.reason == crate::event::PERMISSION_PROMPT
            && (a.needs_user || crate::state::actor_task(&a.role).is_none())
    })
}

/// One role's summary. `live` maps a terminal address to what the
/// program there reports doing; `work` the watch pass's per-role
/// screen classification and `failed` a role's StopFailure error,
/// both keyed `(studio, role)`.
/// One role's summary. `live` maps a terminal address to what the
/// program there reports doing; `work` the watch pass's per-role
/// screen classification and `failed` a role's StopFailure error,
/// both keyed `(studio, role)`.
pub(crate) fn role_summary(
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
    let open = state.open_attention().find(|a| a.role == role);
    Some(RoleSummary {
        role: cap(role),
        name: role_name(state, role),
        terminal: terminal.map(|t| cap(&t)),
        runner: actor.map(|a| cap(&a.runner)),
        model: actor.map(|a| cap(&a.model)),
        effort: actor.and_then(|a| a.effort.clone().filter(|e| !e.is_empty()).map(|e| cap(&e))),
        state: match lifecycle {
            RoleLifecycle::Live => "live",
            RoleLifecycle::Lost => "lost",
            RoleLifecycle::Closed => "closed",
        }
        .into(),
        doing,
        work,
        failed,
        workdir: None,
        task: task.map(|t| cap(&t)),
        priority,
        resumable: lifecycle == RoleLifecycle::Closed && r.session_id.is_some() && !r.held,
        since_ms: r.since_ms,
        attention: role_attention(state, role),
        attention_user: role_attention_user(state, role),
        attention_seq: open.map(|a| a.seq),
        attention_detail: open.map(attention_detail),
    })
}

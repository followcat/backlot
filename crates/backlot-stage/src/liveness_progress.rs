//! Per-role progress alerts (Rule A: looping refusals; Rule B: no progress on coding actors).

use std::collections::BTreeMap;

use backlot_model::event::{LOOPING, NO_PROGRESS, PERMISSION_PROMPT, QUESTION, Verdict};
use backlot_model::state::{Attention, StudioState, Task, TaskStatus, actor_task};

/// 10-minute sliding window for counting identical non-probe guard refusals.
pub(crate) const LOOPING_WINDOW_MS: u64 = 10 * 60 * 1000;
/// Minimum matching refusals within `LOOPING_WINDOW_MS` to raise `looping`.
pub(crate) const LOOPING_THRESHOLD: usize = 5;

/// Task kinds eligible for Rule B (`no progress` work-area check).
pub(crate) const NO_PROGRESS_TASK_KINDS: &[&str] = &[
    "bugfix",
    "feature",
    "fix",
    "refactor",
    "impl",
    "docs",
    "tooling",
    "infra",
    "site",
    "migration",
    "test",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopingAlert {
    pub role: String,
    pub rule: Option<String>,
    pub command: String,
    pub count: usize,
    pub reviewer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NoProgressAlert {
    pub role: String,
    pub task: String,
    pub stretch_start_ms: u64,
    pub active_ms: u64,
    pub threshold_ms: u64,
    pub to: Option<String>,
}

pub(crate) fn is_coding_task_kind(kind: &str) -> bool {
    let lower = kind.to_ascii_lowercase();
    NO_PROGRESS_TASK_KINDS.contains(&lower.as_str())
}

fn reviewer_for_role(state: &StudioState, role: &str) -> Option<String> {
    if let Some(task) = actor_task(role) {
        return Some(
            state
                .tasks
                .get(task)
                .map(crate::notices::reviewer_for)
                .unwrap_or_else(|| "director".into()),
        );
    }
    if role == "producer" {
        return state
            .roles
            .get("director")
            .is_some_and(|r| r.live)
            .then(|| "director".into());
    }
    Some("producer".into())
}

fn find_looping_burst(
    state: &StudioState,
    role: &str,
    cutoff: u64,
    now_ms: u64,
) -> Option<(Option<String>, String, usize)> {
    let mut counts: BTreeMap<(Option<String>, String), (usize, u64)> = BTreeMap::new();
    for r in &state.role_refusals {
        if r.role != role || r.is_probe || r.ts_ms < cutoff || r.ts_ms > now_ms {
            continue;
        }
        let entry = counts
            .entry((r.rule.clone(), r.command.clone()))
            .or_insert((0, 0));
        entry.0 += 1;
        entry.1 = entry.1.max(r.seq);
    }
    counts
        .into_iter()
        .filter(|(_, (n, _))| *n >= LOOPING_THRESHOLD)
        .max_by_key(|(_, (n, seq))| (*n, *seq))
        .map(|((rule, cmd), (n, _))| (rule, cmd, n))
}

/// Evaluate Rule A (`looping`): `>= 5` identical non-probe `role.refused` hits
/// within a 10-minute sliding window, repeating at most once per window.
pub(crate) fn evaluate_looping(
    state: &StudioState,
    role: &str,
    now_ms: u64,
) -> Option<LoopingAlert> {
    let cutoff = now_ms.saturating_sub(LOOPING_WINDOW_MS);
    let recent_loop = state
        .attention
        .iter()
        .filter(|a| a.role == role && a.reason == LOOPING)
        .max_by_key(|a| a.seq);
    if let Some(prev) = recent_loop
        && (prev.open || prev.ts_ms >= cutoff)
    {
        return None;
    }
    let (rule, command, count) = find_looping_burst(state, role, cutoff, now_ms)?;
    Some(LoopingAlert {
        role: role.to_string(),
        rule,
        command,
        count,
        reviewer: reviewer_for_role(state, role),
    })
}

/// Return a clearing reason for an open `looping` attention if the role closed
/// or the 10-minute window no longer contains a repeating burst.
pub(crate) fn looping_clear_due(
    state: &StudioState,
    attn: &Attention,
    now_ms: u64,
) -> Option<&'static str> {
    if !state.roles.get(&attn.role).is_some_and(|r| r.live) {
        return Some("closed");
    }
    if now_ms.saturating_sub(attn.ts_ms) >= LOOPING_WINDOW_MS {
        let cutoff = now_ms.saturating_sub(LOOPING_WINDOW_MS);
        if find_looping_burst(state, &attn.role, cutoff, now_ms).is_none() {
            return Some("window expired");
        }
    }
    None
}

fn task_running_since_ms(t: &Task) -> u64 {
    t.casts
        .iter()
        .map(|c| (c.seq, c.ts_ms))
        .chain(
            t.reviews
                .iter()
                .filter(|r| r.verdict == Verdict::ChangesRequested)
                .map(|r| (r.seq, r.ts_ms)),
        )
        .chain(
            t.state_changes
                .iter()
                .filter(|s| s.resumed_to == Some(TaskStatus::Running))
                .map(|s| (s.seq, s.ts_ms)),
        )
        .max_by_key(|(s, _)| *s)
        .map_or(t.created_ts_ms, |(_, ts)| ts)
}

fn is_wait_excluded_reason(reason: &str) -> bool {
    matches!(
        reason,
        PERMISSION_PROMPT | QUESTION | "rate limited" | "blocked"
    )
}

fn merged_overlap_ms(stretch_start: u64, now_ms: u64, intervals: &[(u64, u64)]) -> u64 {
    let mut clipped: Vec<(u64, u64)> = intervals
        .iter()
        .filter_map(|&(s, e)| {
            let lo = s.max(stretch_start);
            let hi = e.min(now_ms);
            (hi > lo).then_some((lo, hi))
        })
        .collect();
    clipped.sort_unstable();
    let mut total = 0u64;
    let mut cur: Option<(u64, u64)> = None;
    for (lo, hi) in clipped {
        match cur {
            Some((cs, ce)) if lo <= ce => cur = Some((cs, ce.max(hi))),
            Some((cs, ce)) => {
                total += ce - cs;
                cur = Some((lo, hi));
            }
            None => cur = Some((lo, hi)),
        }
    }
    if let Some((cs, ce)) = cur {
        total += ce - cs;
    }
    total
}

fn no_progress_recipient(state: &StudioState, task: &Task) -> Option<String> {
    if state.roles.contains_key("director") {
        Some("director".into())
    } else {
        Some(crate::notices::reviewer_for(task))
    }
}

/// Evaluate Rule B (`no progress`): active coding actor with non-readonly profile
/// and allowlisted `task_kind` running `>= 2 * stall_limit_s` (excluding waits)
/// without a work-area change or new commit. Fires at most once per stretch.
pub(crate) fn evaluate_no_progress(
    state: &StudioState,
    role: &str,
    last_work_area_change_ms: Option<u64>,
    live_blocked_intervals: &[(u64, u64)],
    now_ms: u64,
) -> Option<NoProgressAlert> {
    let change_ms = last_work_area_change_ms?;
    let task_id = actor_task(role)?;
    let task = state.tasks.get(task_id)?;
    if task.status != TaskStatus::Running || !is_coding_task_kind(&task.task_kind) {
        return None;
    }
    if !task
        .cast
        .as_ref()
        .is_some_and(|c| c.actor.permission_profile != "readonly")
    {
        return None;
    }
    if state
        .roles
        .get(role)
        .and_then(|r| r.actor.as_ref())
        .is_some_and(|a| a.permission_profile == "readonly")
    {
        return None;
    }

    let stretch_start_ms = change_ms.max(task_running_since_ms(task));
    if now_ms <= stretch_start_ms {
        return None;
    }
    if state
        .attention
        .iter()
        .any(|a| a.role == role && a.reason == NO_PROGRESS && a.ts_ms >= stretch_start_ms)
    {
        return None;
    }

    let mut excluded: Vec<(u64, u64)> = Vec::from(live_blocked_intervals);
    for a in &state.attention {
        if a.role != role || !is_wait_excluded_reason(&a.reason) {
            continue;
        }
        let end = if a.open {
            now_ms
        } else {
            state
                .attention_cleared_ms
                .get(&a.seq)
                .copied()
                .unwrap_or(a.ts_ms)
        };
        excluded.push((a.ts_ms, end));
    }

    let excluded_ms = merged_overlap_ms(stretch_start_ms, now_ms, &excluded);
    let active_ms = now_ms
        .saturating_sub(stretch_start_ms)
        .saturating_sub(excluded_ms);
    let threshold_ms = u64::from(state.stall_limit_s()).saturating_mul(2_000);
    if active_ms < threshold_ms {
        return None;
    }

    Some(NoProgressAlert {
        role: role.to_string(),
        task: task_id.to_string(),
        stretch_start_ms,
        active_ms,
        threshold_ms,
        to: no_progress_recipient(state, task),
    })
}

/// Clear an open `no progress` attention once the work area changes, the task
/// stops running, or the role is no longer live.
pub(crate) fn no_progress_clear_due(
    state: &StudioState,
    attn: &Attention,
    last_work_area_change_ms: Option<u64>,
) -> Option<&'static str> {
    if !state.roles.get(&attn.role).is_some_and(|r| r.live) {
        return Some("closed");
    }
    if let Some(task_id) = actor_task(&attn.role)
        && state
            .tasks
            .get(task_id)
            .is_some_and(|t| t.status != TaskStatus::Running)
    {
        return Some("task not running");
    }
    if last_work_area_change_ms.is_some_and(|t| t > attn.ts_ms) {
        return Some("work area changed");
    }
    None
}

/// Format the ledger detail & reviewer notice body for a `looping` alert.
pub(crate) fn looping_detail(alert: &LoopingAlert) -> String {
    let rule = alert.rule.as_deref().unwrap_or("guard");
    format!(
        "{} identical `{rule}` guard refusals on `{}` in 10m",
        alert.count, alert.command
    )
}

/// Format the reviewer notice body for an open `looping` attention.
pub(crate) fn looping_notice_body(attention: &Attention) -> String {
    format!(
        "# {} looping\n\n`{}` repeatedly hit the guard: {}.\n\nTell it a safe alternative with `backlot tell {} --text \"...\"`.\n",
        attention.role, attention.role, attention.detail, attention.role,
    )
}

/// Format the ledger detail for a `no progress` alert.
pub(crate) fn no_progress_detail(alert: &NoProgressAlert) -> String {
    let active = backlot_model::event::age_short(alert.active_ms);
    format!(
        "no work-area file change or commit on {} for {active} of active running time",
        alert.task
    )
}

/// Format the director notice body for a low-noise `no progress` alert.
pub(crate) fn no_progress_notice_body(attention: &Attention) -> String {
    format!(
        "# {} no progress\n\n`{}` has had {}. Check whether it needs guidance via `backlot status` or `backlot tell {} --text \"...\"`.\n",
        attention.role, attention.role, attention.detail, attention.role,
    )
}

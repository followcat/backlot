//! Deterministic daily/window studio digest and requirement coverage matrix.

use std::collections::BTreeSet;

use crate::attention::AttentionClass;
use crate::event::{EventKind, TaskStateChange, Verdict};
use crate::metrics::detail::compute_rate_limited;
use crate::metrics::digest_types::{
    BlockedTaskItem, CoverageCheck, CoverageRow, DIGEST_SCHEMA_VERSION, DeliveredItem, DigestInput,
    DigestRateLimitState, DigestReport, FinishItem, OpenAskItem, OpenAttentionItem,
    RateLimitOpenItem, ReturnedItem, StudioDigest, StudioDigestInput, WaitingOnUser,
};
use crate::state::{self, Task, TaskStatus};
use crate::summary_build::open_ask;

fn task_status_label(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Created => "created",
        TaskStatus::Running => "running",
        TaskStatus::Delivered => "delivered",
        TaskStatus::Accepted => "accepted",
        TaskStatus::PartiallyAccepted => "partially_accepted",
        TaskStatus::Paused => "paused",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Abandoned => "abandoned",
        TaskStatus::AtCap => "at_cap",
        TaskStatus::TakenOver => "taken_over",
    }
}

fn verdict_label(v: Verdict) -> &'static str {
    match v {
        Verdict::Accepted => "accepted",
        Verdict::PartiallyAccepted => "partially_accepted",
        Verdict::ChangesRequested => "changes_requested",
        Verdict::Blocked => "blocked",
    }
}

fn parse_design_ref_at(text: &str, idx: usize) -> Option<(u32, String)> {
    if idx > 0
        && text[..idx]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric())
    {
        return None;
    }
    let after = text.get(idx..)?;
    if after.len() < 6 || !after[..6].eq_ignore_ascii_case("design") {
        return None;
    }
    let mut rest = &after[6..];
    let ws1 = rest.len() - rest.trim_start().len();
    if ws1 == 0 {
        return None;
    }
    rest = &rest[ws1..];
    if !(rest.starts_with('v') || rest.starts_with('V')) {
        return None;
    }
    rest = &rest[1..];
    let v_digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if v_digits.is_empty() {
        return None;
    }
    let version: u32 = v_digits.parse().ok()?;
    rest = &rest[v_digits.len()..];
    let ws2 = rest.len() - rest.trim_start().len();
    if ws2 == 0 {
        return None;
    }
    rest = rest[ws2..].strip_prefix('§')?.trim_start();
    let raw_id: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+' | '/'))
        .collect();
    let id = raw_id.trim_end_matches(['.', '-', '/']).to_string();
    if id.is_empty() {
        return None;
    }
    Some((version, id))
}

/// Extract normalized `design v<N> §<id>` references from acceptance markdown.
pub fn extract_design_refs(text: &str) -> Vec<String> {
    let mut found: BTreeSet<(u32, String)> = BTreeSet::new();
    for (idx, _) in text.char_indices() {
        if let Some(parsed) = parse_design_ref_at(text, idx) {
            found.insert(parsed);
        }
    }
    found
        .into_iter()
        .map(|(v, id)| format!("design v{v} §{id}"))
        .collect()
}

fn ref_sort_key(section: Option<&str>) -> (bool, u32, String) {
    match section {
        Some(s) => {
            let v = s
                .strip_prefix("design v")
                .and_then(|rest| rest.split(' ').next())
                .and_then(|digits| digits.parse::<u32>().ok())
                .unwrap_or(u32::MAX);
            (false, v, s.to_string())
        }
        None => (true, u32::MAX, String::new()),
    }
}

fn task_checks_row(task: &Task, studio_checks: &[String]) -> (Vec<CoverageCheck>, String) {
    if studio_checks.is_empty() && task.checked.is_empty() {
        return (Vec::new(), "checks: none".to_string());
    }
    let target_deliv = task.deliveries.last().map(|d| d.n);
    let mut names: Vec<String> = studio_checks.to_vec();
    for c in &task.checked {
        if !names.iter().any(|n| n == &c.name) {
            names.push(c.name.clone());
        }
    }
    let checks: Vec<CoverageCheck> = names
        .into_iter()
        .map(|name| {
            let rec = target_deliv
                .and_then(|dn| {
                    task.checked
                        .iter()
                        .rfind(|c| c.name == name && c.delivery == dn)
                })
                .or_else(|| task.checked.iter().rfind(|c| c.name == name));
            let status = match rec {
                Some(r) if r.passed() => "pass",
                Some(r) if r.timed_out => "timeout",
                Some(_) => "fail",
                None => "-",
            };
            CoverageCheck {
                name,
                status: status.to_string(),
            }
        })
        .collect();
    let summary = checks
        .iter()
        .map(|c| format!("{}:{}", c.name, c.status))
        .collect::<Vec<_>>()
        .join(", ");
    (checks, summary)
}

fn digest_studio(input: &StudioDigestInput, since_ms: u64) -> StudioDigest {
    let state = state::replay(&input.events);
    let unknown_events = input
        .events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::Unknown { .. }))
        .count();

    let checks: Vec<String> = state
        .studio
        .as_ref()
        .map(|s| s.checks.iter().map(|c| c.name.clone()).collect())
        .unwrap_or_default();
    let checks_status = if checks.is_empty() {
        "checks: none".to_string()
    } else {
        format!("{} configured ({})", checks.len(), checks.join(", "))
    };
    let studio_finished_ts_ms = state
        .finished
        .as_ref()
        .filter(|f| f.ts_ms >= since_ms)
        .map(|f| f.ts_ms);

    let mut finished = Vec::new();
    let mut delivered = Vec::new();
    let mut sent_back = Vec::new();
    let mut blocked = Vec::new();
    let mut coverage = Vec::new();

    for t in state.tasks.values() {
        if t.status.is_terminal()
            && let Some(ts_ms) = t.closed_ts_ms
            && ts_ms >= since_ms
        {
            finished.push(FinishItem {
                task: t.id.clone(),
                title: t.title.clone(),
                status: task_status_label(t.status).to_string(),
                ts_ms,
            });
        }
        for d in &t.deliveries {
            if d.ts_ms >= since_ms {
                delivered.push(DeliveredItem {
                    task: t.id.clone(),
                    title: t.title.clone(),
                    delivery: d.n,
                    commits: (!d.commits.is_empty()).then(|| d.commits.clone()),
                    ts_ms: d.ts_ms,
                });
            }
        }
        for r in &t.reviews {
            if r.verdict == Verdict::ChangesRequested && r.ts_ms >= since_ms {
                sent_back.push(ReturnedItem {
                    task: t.id.clone(),
                    delivery: r.delivery,
                    reviewer: r.reviewer.clone(),
                    note: (!r.conclusion.is_empty()).then(|| r.conclusion.clone()),
                    ts_ms: r.ts_ms,
                });
            }
        }
        if t.status == TaskStatus::Blocked {
            let reason = t
                .state_changes
                .iter()
                .rfind(|sc| sc.state == TaskStateChange::Blocked)
                .map(|sc| sc.reason.clone())
                .filter(|s| !s.is_empty());
            let waits_for = t
                .dependencies
                .iter()
                .filter(|d| {
                    state
                        .tasks
                        .get(*d)
                        .is_some_and(|dt| !dt.status.is_terminal())
                })
                .cloned()
                .collect();
            blocked.push(BlockedTaskItem {
                task: t.id.clone(),
                title: t.title.clone(),
                reason,
                waits_for,
            });
        }

        let commits = t
            .deliveries
            .last()
            .map(|d| d.commits.clone())
            .filter(|c| !c.is_empty());
        let (cov_checks, checks_summary) = task_checks_row(t, &checks);
        let verdict = t
            .reviews
            .last()
            .map(|r| verdict_label(r.verdict))
            .unwrap_or_else(|| task_status_label(t.status))
            .to_string();
        let refs = input
            .acceptance_texts
            .get(&t.acceptance)
            .map(|txt| extract_design_refs(txt))
            .unwrap_or_default();
        if refs.is_empty() {
            coverage.push(CoverageRow {
                design_section: None,
                task: t.id.clone(),
                task_title: t.title.clone(),
                commits,
                checks: cov_checks,
                checks_summary,
                verdict,
            });
        } else {
            for r in refs {
                coverage.push(CoverageRow {
                    design_section: Some(r),
                    task: t.id.clone(),
                    task_title: t.title.clone(),
                    commits: commits.clone(),
                    checks: cov_checks.clone(),
                    checks_summary: checks_summary.clone(),
                    verdict: verdict.clone(),
                });
            }
        }
    }

    finished.sort_by_key(|f| (f.ts_ms, f.task.clone()));
    delivered.sort_by_key(|d| (d.ts_ms, d.task.clone(), d.delivery));
    sent_back.sort_by_key(|s| (s.ts_ms, s.task.clone(), s.delivery));
    coverage.sort_by(|a, b| {
        ref_sort_key(a.design_section.as_deref())
            .cmp(&ref_sort_key(b.design_section.as_deref()))
            .then_with(|| a.task.cmp(&b.task))
    });

    let mut open_attentions: Vec<OpenAttentionItem> = state
        .open_attention()
        .filter(|a| AttentionClass::of(&a.reason, a.needs_user) == AttentionClass::NeedsYou)
        .map(|a| OpenAttentionItem {
            role: a.role.clone(),
            reason: a.reason.clone(),
            detail: a.detail.clone(),
            ts_ms: a.ts_ms,
        })
        .collect();
    open_attentions.sort_by_key(|a| (a.ts_ms, a.role.clone()));

    let mut roles: BTreeSet<&str> = ["producer", "director", "writer"].into_iter().collect();
    for r in state.roles.keys() {
        roles.insert(r.as_str());
    }
    let mut asks: Vec<OpenAskItem> = roles
        .into_iter()
        .filter_map(|role| {
            open_ask(&state, role).map(|m| OpenAskItem {
                role: role.to_string(),
                seq: m.seq,
                ts_ms: m.ts_ms,
            })
        })
        .collect();
    asks.sort_by_key(|a| (a.ts_ms, a.seq));

    let design_waiting = state
        .design
        .as_ref()
        .is_some_and(|d| d.phase.is_waiting_user());

    let mut rl_open: Vec<RateLimitOpenItem> = state
        .open_attention()
        .filter(|a| a.reason == "rate limited")
        .map(|a| RateLimitOpenItem {
            role: a.role.clone(),
            ts_ms: a.ts_ms,
            reset_at_ms: a.reset_at_ms,
        })
        .collect();
    rl_open.sort_by_key(|r| (r.ts_ms, r.role.clone()));
    let rl_window = compute_rate_limited(&state.attention, Some(since_ms));

    StudioDigest {
        studio: input.studio.clone(),
        unknown_events,
        checks,
        checks_status,
        studio_finished_ts_ms,
        finished,
        delivered,
        sent_back,
        blocked,
        waiting_on_user: WaitingOnUser {
            attentions: open_attentions,
            asks,
            design_waiting,
        },
        rate_limits: DigestRateLimitState {
            open: rl_open,
            events_in_window: rl_window.total,
            windows_in_window: rl_window.windows,
            max_per_window: rl_window.max_per_window,
        },
        coverage,
    }
}

/// Build the deterministic digest and coverage matrix across studios.
pub fn compute_digest(input: &DigestInput) -> DigestReport {
    let mut studios: Vec<StudioDigest> = input
        .studios
        .iter()
        .map(|s| digest_studio(s, input.since_ms))
        .collect();
    studios.sort_by(|a, b| a.studio.cmp(&b.studio));
    let unknown_events_skipped = studios.iter().map(|s| s.unknown_events).sum();
    DigestReport {
        schema_version: DIGEST_SCHEMA_VERSION,
        since_ms: input.since_ms,
        now_ms: input.now_ms,
        unknown_events_skipped,
        studios,
    }
}

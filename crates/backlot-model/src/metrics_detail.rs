//! Attention classes, rate-limit clustering, approvals, guard refusals,
//! and Hollywood notice counters over replayed studio ledgers.

use std::collections::BTreeMap;

use crate::attention::AttentionClass;
use crate::event::{ApprovalDecision, Event, EventKind, PERMISSION_PROMPT};
use crate::hollywood::HollywoodState;
use crate::metrics::in_window;
use crate::metrics::ratio;
use crate::metrics::types::{
    ApprovalMetrics, AttentionMetrics, HollywoodMetrics, RateLimitedMetrics, RefusedMetrics,
    RefusedRuleCounts,
};
use crate::state::{Attention, StudioState};

const SAME_RESET_WINDOW_MS: u64 = 60_000;
const FALLBACK_GAP_MS: u64 = 600_000;
const LABELED_FALLBACK_GAP_MS: u64 = 6 * 3_600_000;

struct RateCluster<'a> {
    anchor_reset_ms: Option<u64>,
    token: Option<&'a str>,
    last_ts_ms: u64,
    count: usize,
}

/// Fold raised/cleared attentions in the selected window, bounding still-open
/// raises by `finished_ts` (when the studio finished) or `now_ms`.
pub(crate) fn compute_attention(
    state: &StudioState,
    events: &[Event],
    since_ms: Option<u64>,
    now_ms: u64,
) -> AttentionMetrics {
    let mut cleared_ts: BTreeMap<u64, u64> = BTreeMap::new();
    for e in events {
        if let EventKind::AttentionCleared { target, .. } = &e.kind {
            cleared_ts.entry(*target).or_insert(e.ts_ms);
        }
    }
    let finished_ts = state.finished.as_ref().map(|f| f.ts_ms);
    let mut out = AttentionMetrics::default();

    for a in &state.attention {
        if !in_window(a.ts_ms, since_ms) {
            continue;
        }
        let end_ms = if a.open {
            finished_ts.map_or(now_ms, |f| now_ms.min(f))
        } else {
            cleared_ts
                .get(&a.seq)
                .copied()
                .or(finished_ts)
                .unwrap_or(a.ts_ms)
        };
        let dur = end_ms.saturating_sub(a.ts_ms);

        out.total += 1;
        out.open += usize::from(a.open);
        out.open_ms += dur;
        if a.needs_user {
            out.needs_user_count += 1;
        }
        if a.reason == PERMISSION_PROMPT && !a.needs_user {
            out.permission_prompts_not_needs_user.count += 1;
            out.permission_prompts_not_needs_user.open_ms += dur;
        }

        let bucket = match AttentionClass::of(&a.reason, a.needs_user) {
            AttentionClass::NeedsYou => &mut out.by_class.needs_you,
            AttentionClass::Stuck => &mut out.by_class.stuck,
            AttentionClass::Info => &mut out.by_class.info,
        };
        bucket.count += 1;
        bucket.open += usize::from(a.open);
        bucket.open_ms += dur;
    }

    out.rate_limited = compute_rate_limited(&state.attention, since_ms);
    out
}

fn waiting_until_token(detail: &str) -> Option<&str> {
    let idx = detail.find("waiting until ")?;
    let rest = &detail[idx + "waiting until ".len()..];
    let token = rest
        .split_whitespace()
        .next()?
        .trim_matches([',', ';', '.', ')', '(']);
    (!token.is_empty()).then_some(token)
}

/// Cluster `"rate limited"` attentions per `(role, reset_window)`.
pub(crate) fn compute_rate_limited(
    attentions: &[Attention],
    since_ms: Option<u64>,
) -> RateLimitedMetrics {
    let mut by_role: BTreeMap<&str, Vec<&Attention>> = BTreeMap::new();
    let mut total = 0usize;
    for a in attentions {
        if a.reason == "rate limited" && in_window(a.ts_ms, since_ms) {
            total += 1;
            by_role.entry(a.role.as_str()).or_default().push(a);
        }
    }

    let mut windows = 0usize;
    let mut max_per_window = 0usize;

    for (_role, mut items) in by_role {
        items.sort_by_key(|a| (a.ts_ms, a.seq));
        let mut clusters: Vec<RateCluster<'_>> = Vec::new();
        for a in items {
            let tok = waiting_until_token(&a.detail);
            let same = if let Some(last) = clusters.last_mut() {
                let fits = match (last.anchor_reset_ms, a.reset_at_ms) {
                    (Some(r1), Some(r2)) => r1.abs_diff(r2) <= SAME_RESET_WINDOW_MS,
                    (Some(r1), None) => a.ts_ms <= r1.saturating_add(SAME_RESET_WINDOW_MS),
                    (None, Some(r2)) => {
                        let ok = a.ts_ms.saturating_sub(last.last_ts_ms) <= FALLBACK_GAP_MS
                            && r2 > last.last_ts_ms
                            && (tok.is_none() || last.token.is_none() || tok == last.token);
                        if ok {
                            last.anchor_reset_ms = Some(r2);
                        }
                        ok
                    }
                    (None, None) => match (last.token, tok) {
                        (Some(t1), Some(t2)) => {
                            t1 == t2
                                && a.ts_ms.saturating_sub(last.last_ts_ms)
                                    <= LABELED_FALLBACK_GAP_MS
                        }
                        _ => a.ts_ms.saturating_sub(last.last_ts_ms) <= FALLBACK_GAP_MS,
                    },
                };
                if fits {
                    last.last_ts_ms = a.ts_ms;
                    if last.token.is_none() {
                        last.token = tok;
                    }
                    last.count += 1;
                    true
                } else {
                    false
                }
            } else {
                false
            };

            if !same {
                clusters.push(RateCluster {
                    anchor_reset_ms: a.reset_at_ms,
                    token: tok,
                    last_ts_ms: a.ts_ms,
                    count: 1,
                });
            }
        }
        windows += clusters.len();
        for c in clusters {
            max_per_window = max_per_window.max(c.count);
        }
    }

    RateLimitedMetrics {
        total,
        windows,
        max_per_window,
        avg_per_window: ratio(total, windows),
    }
}

/// Fold `role.approval` decisions within `[since_ms, ..]`.
pub(crate) fn compute_approvals(state: &StudioState, since_ms: Option<u64>) -> ApprovalMetrics {
    let mut out = ApprovalMetrics::default();
    for a in &state.approvals {
        if !in_window(a.ts_ms, since_ms) {
            continue;
        }
        out.total += 1;
        match a.decision {
            ApprovalDecision::Allow => out.allow += 1,
            ApprovalDecision::Deny => out.deny += 1,
        }
    }
    out
}

/// Commands dropped as benign read-only git discovery noise by PR #209.
pub(crate) fn is_209_probe(command: &str, argv: &[String]) -> bool {
    let trimmed = command.trim();
    if trimmed == "git rev-parse --show-toplevel" || trimmed == "git --version" {
        return true;
    }
    let mut args = argv;
    if args.first().map(String::as_str) != Some("git") {
        return false;
    }
    args = &args[1..];
    while args.len() >= 2 && args[0] == "-C" {
        args = &args[2..];
    }
    matches!(
        args.iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice(),
        ["--version"] | ["rev-parse", "--show-toplevel"]
    )
}

/// Count `role.refused` events with and without #209 probe commands, per rule.
pub(crate) fn compute_refused(events: &[Event], since_ms: Option<u64>) -> RefusedMetrics {
    let mut out = RefusedMetrics::default();
    for e in events {
        if !in_window(e.ts_ms, since_ms) {
            continue;
        }
        let EventKind::RoleRefused {
            command,
            argv,
            rule,
            ..
        } = &e.kind
        else {
            continue;
        };
        let probe = is_209_probe(command, argv);
        out.total += 1;
        if probe {
            out.probes_dropped_by_209 += 1;
        } else {
            out.without_probes += 1;
        }
        let key = rule.clone().unwrap_or_else(|| "unspecified".to_string());
        let entry = out.by_rule.entry(key).or_default();
        entry.total += 1;
        if !probe {
            entry.without_probes += 1;
        }
    }
    out
}

/// Compute Stage -> Hollywood notice delivery and drop rate for one studio
/// (`studio: Some(name)`) or across all notices (`studio: None`).
pub(crate) fn compute_hollywood(
    hstate: &HollywoodState,
    studio: Option<&str>,
    since_ms: Option<u64>,
) -> HollywoodMetrics {
    let mut sent = 0usize;
    let mut dropped = 0usize;
    for m in &hstate.messages {
        if m.from != "stage" || m.to != "hollywood" || !in_window(m.ts_ms, since_ms) {
            continue;
        }
        if let Some(target_studio) = studio
            && m.studio.as_deref() != Some(target_studio)
        {
            continue;
        }
        sent += 1;
        if m.dropped {
            dropped += 1;
        }
    }
    HollywoodMetrics {
        notices_sent: sent,
        notices_dropped: dropped,
        drop_rate: ratio(dropped, sent),
    }
}

/// Merge one studio's rule map into an aggregate `RefusedRuleCounts` map.
pub(crate) fn merge_rule_counts(
    dst: &mut BTreeMap<String, RefusedRuleCounts>,
    src: &BTreeMap<String, RefusedRuleCounts>,
) {
    for (rule, counts) in src {
        let entry = dst.entry(rule.clone()).or_default();
        entry.total += counts.total;
        entry.without_probes += counts.without_probes;
    }
}

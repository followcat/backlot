//! Read-only studio metrics and rate/percentile calculations over ledgers.
//!
//! Pure functions only: callers load events and pass wall-clock `now_ms`.

use crate::event::{EventKind, Verdict};
use crate::hollywood;
use crate::state::{self, actor_task};

#[path = "metrics_detail.rs"]
mod detail;
#[path = "metrics_digest.rs"]
pub mod digest;
#[path = "metrics_digest_types.rs"]
mod digest_types;
#[path = "metrics_timing.rs"]
mod timing;
#[path = "metrics_types.rs"]
mod types;

pub use digest::{compute_digest, extract_design_refs};
pub use digest_types::{
    BlockedTaskItem, CoverageCheck, CoverageRow, DIGEST_SCHEMA_VERSION, DeliveredItem, DigestInput,
    DigestRateLimitState, DigestReport, FinishItem, OpenAskItem, OpenAttentionItem,
    RateLimitOpenItem, ReturnedItem, StudioDigest, StudioDigestInput, WaitingOnUser,
};
pub use types::{
    AggregateMetrics, AggregateTimingMetrics, ApprovalMetrics, AttentionByClass,
    AttentionClassBucket, AttentionMetrics, ChecksCounts, ContinuityMetrics, CountOpenMs,
    FindingsMetrics, HollywoodMetrics, METRICS_SCHEMA_VERSION, MetricsInput, MetricsReport,
    QualityMetrics, RateLimitedMetrics, RefusedMetrics, RefusedRuleCounts, StudioChecksMetrics,
    StudioEventsInput, StudioMetrics, StudioTimingMetrics,
};

/// Check whether `ts_ms` falls at or after `since_ms`.
pub(crate) fn in_window(ts_ms: u64, since_ms: Option<u64>) -> bool {
    since_ms.is_none_or(|cutoff| ts_ms >= cutoff)
}

/// Safe `num / den` returning `None` on zero denominator.
pub(crate) fn ratio(num: usize, den: usize) -> Option<f64> {
    (den > 0).then(|| num as f64 / den as f64)
}

fn compute_studio(
    input: &StudioEventsInput,
    hstate: &hollywood::HollywoodState,
    since_ms: Option<u64>,
    now_ms: u64,
) -> (StudioMetrics, timing::StudioTimingSamples) {
    let state = state::replay(&input.events);
    let unknown_events = input
        .events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::Unknown { .. }))
        .count();
    let events_total = input.events.len().saturating_sub(unknown_events);
    let events_in_window = input
        .events
        .iter()
        .filter(|e| !matches!(e.kind, EventKind::Unknown { .. }) && in_window(e.ts_ms, since_ms))
        .count();

    let first_dispatches = timing::first_dispatch_by_req_version(&state, &input.events);
    let mut quality = QualityMetrics::default();
    let configured_checks = state.studio.as_ref().map_or(0, |s| s.checks.len());
    let mut checks = StudioChecksMetrics {
        configured: configured_checks > 0,
        configured_checks,
        ..Default::default()
    };
    let mut findings = FindingsMetrics::default();

    for t in state.tasks.values() {
        if in_window(t.created_ts_ms, since_ms) {
            quality.created_tasks += 1;
            if first_dispatches
                .get(&t.requirement_version)
                .is_some_and(|&dseq| t.created_seq < dseq)
            {
                quality.early_tasks += 1;
            }
        }
        if let Some(d1) = t.deliveries.first()
            && in_window(d1.ts_ms, since_ms)
        {
            quality.delivered_tasks += 1;
            if t.reviews
                .iter()
                .any(|r| r.delivery == d1.n && r.verdict == Verdict::Accepted)
            {
                quality.first_delivery_accepted += 1;
            }
        }
        for d in &t.deliveries {
            if in_window(d.ts_ms, since_ms) {
                quality.deliveries += 1;
            }
        }
        for r in &t.reviews {
            if r.verdict == Verdict::ChangesRequested && in_window(r.ts_ms, since_ms) {
                quality.changes_requested_reviews += 1;
            }
        }
        for c in &t.checked {
            if in_window(c.ts_ms, since_ms) {
                checks.task_checked += 1;
                if c.passed() {
                    checks.task_checked_passed += 1;
                } else {
                    checks.task_checked_failed += 1;
                }
            }
        }
        for fr in &t.finding_reports {
            if in_window(fr.ts_ms, since_ms) {
                findings.task_finding_events += 1;
                findings.findings += fr.findings.len();
            }
        }
    }
    quality.first_delivery_pass_rate =
        ratio(quality.first_delivery_accepted, quality.delivered_tasks);
    quality.changes_requested_rate = ratio(quality.changes_requested_reviews, quality.deliveries);

    let tells = state
        .messages
        .iter()
        .filter(|m| {
            m.from == "director"
                && actor_task(&m.to).is_some()
                && m.purpose.is_none()
                && m.source_seq.is_none()
                && in_window(m.ts_ms, since_ms)
        })
        .count();
    let continuity = ContinuityMetrics {
        director_actor_tells: tells,
        created_tasks: quality.created_tasks,
        tells_per_task: ratio(tells, quality.created_tasks),
    };

    let (timing_metrics, timing_samples) = timing::compute_timing(&state, &input.events, since_ms);
    let metrics = StudioMetrics {
        studio: input.studio.clone(),
        events_total,
        events_in_window,
        unknown_events,
        quality,
        checks,
        findings,
        continuity,
        attention: detail::compute_attention(&state, &input.events, since_ms, now_ms),
        approvals: detail::compute_approvals(&state, since_ms),
        refused: detail::compute_refused(&input.events, since_ms),
        hollywood: detail::compute_hollywood(hstate, Some(&input.studio), since_ms),
        timing: timing_metrics,
    };
    (metrics, timing_samples)
}

fn aggregate(
    studios: &[StudioMetrics],
    samples: &[timing::StudioTimingSamples],
    hstate: &hollywood::HollywoodState,
    hollywood_include_unscoped: bool,
    since_ms: Option<u64>,
) -> AggregateMetrics {
    let mut all = AggregateMetrics {
        studios_total: studios.len(),
        ..Default::default()
    };
    let mut opening_ms = Vec::new();
    let mut review_ms = Vec::new();
    let mut wall_ms = Vec::new();

    for (s, smp) in studios.iter().zip(samples.iter()) {
        all.studios_with_checks += usize::from(s.checks.configured);
        all.events_total += s.events_total;
        all.events_in_window += s.events_in_window;
        all.unknown_events += s.unknown_events;

        all.quality.delivered_tasks += s.quality.delivered_tasks;
        all.quality.first_delivery_accepted += s.quality.first_delivery_accepted;
        all.quality.deliveries += s.quality.deliveries;
        all.quality.changes_requested_reviews += s.quality.changes_requested_reviews;
        all.quality.created_tasks += s.quality.created_tasks;
        all.quality.early_tasks += s.quality.early_tasks;

        all.checks.task_checked += s.checks.task_checked;
        all.checks.task_checked_passed += s.checks.task_checked_passed;
        all.checks.task_checked_failed += s.checks.task_checked_failed;

        all.findings.task_finding_events += s.findings.task_finding_events;
        all.findings.findings += s.findings.findings;
        all.continuity.director_actor_tells += s.continuity.director_actor_tells;
        all.continuity.created_tasks += s.continuity.created_tasks;

        all.attention.total += s.attention.total;
        all.attention.open += s.attention.open;
        all.attention.open_ms += s.attention.open_ms;
        all.attention.needs_user_count += s.attention.needs_user_count;
        all.attention.permission_prompts_not_needs_user.count +=
            s.attention.permission_prompts_not_needs_user.count;
        all.attention.permission_prompts_not_needs_user.open_ms +=
            s.attention.permission_prompts_not_needs_user.open_ms;

        for (dst, src) in [
            (
                &mut all.attention.by_class.needs_you,
                &s.attention.by_class.needs_you,
            ),
            (
                &mut all.attention.by_class.stuck,
                &s.attention.by_class.stuck,
            ),
            (&mut all.attention.by_class.info, &s.attention.by_class.info),
        ] {
            dst.count += src.count;
            dst.open += src.open;
            dst.open_ms += src.open_ms;
        }

        all.attention.rate_limited.total += s.attention.rate_limited.total;
        all.attention.rate_limited.windows += s.attention.rate_limited.windows;
        all.attention.rate_limited.max_per_window = all
            .attention
            .rate_limited
            .max_per_window
            .max(s.attention.rate_limited.max_per_window);

        all.approvals.total += s.approvals.total;
        all.approvals.allow += s.approvals.allow;
        all.approvals.deny += s.approvals.deny;

        all.refused.total += s.refused.total;
        all.refused.without_probes += s.refused.without_probes;
        all.refused.probes_dropped_by_209 += s.refused.probes_dropped_by_209;
        detail::merge_rule_counts(&mut all.refused.by_rule, &s.refused.by_rule);

        if let Some(o) = smp.opening_ms {
            opening_ms.push(o);
        }
        review_ms.extend_from_slice(&smp.review_latencies_ms);
        wall_ms.extend_from_slice(&smp.wall_clocks_ms);
        all.timing.pause_clusters += smp.pause_clusters;
        all.timing.paused_tasks_in_clusters += smp.paused_tasks_in_clusters;

        if !hollywood_include_unscoped {
            all.hollywood.notices_sent += s.hollywood.notices_sent;
            all.hollywood.notices_dropped += s.hollywood.notices_dropped;
        }
    }

    all.checks_coverage_rate = ratio(all.studios_with_checks, all.studios_total);
    all.quality.first_delivery_pass_rate = ratio(
        all.quality.first_delivery_accepted,
        all.quality.delivered_tasks,
    );
    all.quality.changes_requested_rate = ratio(
        all.quality.changes_requested_reviews,
        all.quality.deliveries,
    );
    all.continuity.tells_per_task = ratio(
        all.continuity.director_actor_tells,
        all.continuity.created_tasks,
    );
    all.attention.rate_limited.avg_per_window = ratio(
        all.attention.rate_limited.total,
        all.attention.rate_limited.windows,
    );

    if hollywood_include_unscoped {
        all.hollywood = detail::compute_hollywood(hstate, None, since_ms);
    } else {
        all.hollywood.drop_rate = ratio(all.hollywood.notices_dropped, all.hollywood.notices_sent);
    }

    all.timing.opening_samples = opening_ms.len();
    all.timing.opening_p50_ms = timing::percentile_ms(&opening_ms, 0.50);
    all.timing.review_latency_samples = review_ms.len();
    all.timing.review_latency_p50_ms = timing::percentile_ms(&review_ms, 0.50);
    all.timing.review_latency_p90_ms = timing::percentile_ms(&review_ms, 0.90);
    all.timing.task_wall_clock_samples = wall_ms.len();
    all.timing.task_wall_clock_p50_ms = timing::percentile_ms(&wall_ms, 0.50);
    all
}

/// Compute exact metrics across one or more studios and Hollywood's ledger.
pub fn compute_metrics(input: &MetricsInput) -> MetricsReport {
    let hstate = hollywood::replay(&input.hollywood_events);
    let mut pairs: Vec<(StudioMetrics, timing::StudioTimingSamples)> = input
        .studios
        .iter()
        .map(|s| compute_studio(s, &hstate, input.since_ms, input.now_ms))
        .collect();
    pairs.sort_by(|a, b| a.0.studio.cmp(&b.0.studio));

    let studios: Vec<StudioMetrics> = pairs.iter().map(|(m, _)| m.clone()).collect();
    let samples: Vec<timing::StudioTimingSamples> = pairs.into_iter().map(|(_, s)| s).collect();
    let all = aggregate(
        &studios,
        &samples,
        &hstate,
        input.hollywood_include_unscoped,
        input.since_ms,
    );

    MetricsReport {
        schema_version: METRICS_SCHEMA_VERSION,
        since_ms: input.since_ms,
        now_ms: input.now_ms,
        unknown_events_skipped: all.unknown_events,
        studios,
        all,
    }
}

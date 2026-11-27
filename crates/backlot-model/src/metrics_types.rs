//! Public types for `backlot metrics` output and input parameters.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::event::Event;
use crate::hollywood::HollywoodEvent;

/// Schema version for `MetricsReport` JSON output.
pub const METRICS_SCHEMA_VERSION: u32 = 1;

/// Read-only input for `compute_metrics`.
#[derive(Debug, Clone)]
pub struct MetricsInput {
    pub studios: Vec<StudioEventsInput>,
    pub hollywood_events: Vec<HollywoodEvent>,
    /// When true (`--all`), aggregate Hollywood stats include unscoped stage notices.
    pub hollywood_include_unscoped: bool,
    /// Inclusive lower bound in ms (`--since`); `None` spans the full ledger.
    pub since_ms: Option<u64>,
    /// Current wall-clock timestamp in ms for open-attention duration bounds.
    pub now_ms: u64,
}

/// One studio's raw ledger events.
#[derive(Debug, Clone)]
pub struct StudioEventsInput {
    pub studio: String,
    pub events: Vec<Event>,
}

/// Top-level metrics document returned by `backlot metrics --json`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricsReport {
    pub schema_version: u32,
    pub since_ms: Option<u64>,
    pub now_ms: u64,
    pub unknown_events_skipped: usize,
    pub studios: Vec<StudioMetrics>,
    pub all: AggregateMetrics,
}

/// Per-studio metrics document.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct StudioMetrics {
    pub studio: String,
    pub events_total: usize,
    pub events_in_window: usize,
    pub unknown_events: usize,
    pub quality: QualityMetrics,
    pub checks: StudioChecksMetrics,
    pub findings: FindingsMetrics,
    pub continuity: ContinuityMetrics,
    pub attention: AttentionMetrics,
    pub approvals: ApprovalMetrics,
    pub refused: RefusedMetrics,
    pub hollywood: HollywoodMetrics,
    pub timing: StudioTimingMetrics,
    pub halts: HaltMetrics,
}

/// Cross-studio aggregate metrics document.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct AggregateMetrics {
    pub studios_total: usize,
    pub studios_with_checks: usize,
    pub checks_coverage_rate: Option<f64>,
    pub events_total: usize,
    pub events_in_window: usize,
    pub unknown_events: usize,
    pub quality: QualityMetrics,
    pub checks: ChecksCounts,
    pub findings: FindingsMetrics,
    pub continuity: ContinuityMetrics,
    pub attention: AttentionMetrics,
    pub approvals: ApprovalMetrics,
    pub refused: RefusedMetrics,
    pub hollywood: HollywoodMetrics,
    pub timing: AggregateTimingMetrics,
    pub halts: HaltMetrics,
}

/// Delivery, review, and early-task quality metrics.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct QualityMetrics {
    pub delivered_tasks: usize,
    pub first_delivery_accepted: usize,
    pub first_delivery_pass_rate: Option<f64>,
    pub deliveries: usize,
    pub changes_requested_reviews: usize,
    pub changes_requested_rate: Option<f64>,
    pub created_tasks: usize,
    pub early_tasks: usize,
}

/// Per-studio configured checks and execution counts.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct StudioChecksMetrics {
    pub configured: bool,
    pub configured_checks: usize,
    pub task_checked: usize,
    pub task_checked_passed: usize,
    pub task_checked_failed: usize,
}

/// Aggregate check execution counts.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ChecksCounts {
    pub task_checked: usize,
    pub task_checked_passed: usize,
    pub task_checked_failed: usize,
}

/// Review/check finding report counts.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FindingsMetrics {
    pub task_finding_events: usize,
    pub findings: usize,
}

/// Director-to-actor plain-tell continuity metrics.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ContinuityMetrics {
    pub director_actor_tells: usize,
    pub created_tasks: usize,
    pub tells_per_task: Option<f64>,
}

/// Attention metrics broken down by `AttentionClass`, prompt flags, and rate limits.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct AttentionMetrics {
    pub total: usize,
    pub open: usize,
    pub open_ms: u64,
    pub by_class: AttentionByClass,
    pub needs_user_count: usize,
    pub permission_prompts_not_needs_user: CountOpenMs,
    pub rate_limited: RateLimitedMetrics,
}

/// Counts and open time per `AttentionClass`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct AttentionByClass {
    pub needs_you: AttentionClassBucket,
    pub stuck: AttentionClassBucket,
    pub info: AttentionClassBucket,
}

/// Count, currently open count, and total open time in one `AttentionClass`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct AttentionClassBucket {
    pub count: usize,
    pub open: usize,
    pub open_ms: u64,
}

/// Count and total open time for a subset of attentions.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CountOpenMs {
    pub count: usize,
    pub open_ms: u64,
}

/// Rate-limit attention counts clustered per `(role, reset_window)`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RateLimitedMetrics {
    pub total: usize,
    pub windows: usize,
    pub max_per_window: usize,
    pub avg_per_window: Option<f64>,
}

/// Permission approval decisions (`role.approval`).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ApprovalMetrics {
    pub total: usize,
    pub allow: usize,
    pub deny: usize,
}

/// Guard refusal counts (`role.refused`), with and without issue-#209 probes.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RefusedMetrics {
    pub total: usize,
    pub without_probes: usize,
    pub probes_dropped_by_209: usize,
    pub by_rule: BTreeMap<String, RefusedRuleCounts>,
}

/// Per-rule guard refusal counts.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RefusedRuleCounts {
    pub total: usize,
    pub without_probes: usize,
}

/// Stage -> Hollywood notice delivery and stale-drop counts.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct HollywoodMetrics {
    pub notices_sent: usize,
    pub notices_dropped: usize,
    pub drop_rate: Option<f64>,
}

/// Per-studio timing and pause-cluster metrics.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct StudioTimingMetrics {
    pub opening_ms: Option<u64>,
    pub review_latency_p50_ms: Option<u64>,
    pub review_latency_p90_ms: Option<u64>,
    pub review_latency_samples: usize,
    pub task_wall_clock_p50_ms: Option<u64>,
    pub task_wall_clock_samples: usize,
    pub pause_clusters: usize,
    pub paused_tasks_in_clusters: usize,
}

/// Aggregate timing metrics pooled across studios.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct AggregateTimingMetrics {
    pub opening_p50_ms: Option<u64>,
    pub opening_samples: usize,
    pub review_latency_p50_ms: Option<u64>,
    pub review_latency_p90_ms: Option<u64>,
    pub review_latency_samples: usize,
    pub task_wall_clock_p50_ms: Option<u64>,
    pub task_wall_clock_samples: usize,
    pub pause_clusters: usize,
    pub paused_tasks_in_clusters: usize,
}

/// Explicit halt/resume counts, false-positive share, and halted duration.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct HaltMetrics {
    pub halts: usize,
    pub resumes: usize,
    pub false_positives: usize,
    pub false_positive_share: Option<f64>,
    pub halted_ms: u64,
}

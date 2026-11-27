//! Public types for `backlot digest` output and input parameters.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::event::Event;
use crate::hollywood::HollywoodEvent;

/// Schema version for `DigestReport` JSON output.
pub const DIGEST_SCHEMA_VERSION: u32 = 1;

/// Input for `compute_digest`.
#[derive(Debug, Clone)]
pub struct DigestInput {
    pub studios: Vec<StudioDigestInput>,
    pub hollywood_events: Vec<HollywoodEvent>,
    pub since_ms: u64,
    pub now_ms: u64,
}

/// One studio's events and acceptance file contents loaded by the caller.
#[derive(Debug, Clone)]
pub struct StudioDigestInput {
    pub studio: String,
    pub events: Vec<Event>,
    /// Keyed by `task.acceptance` path as recorded in `task.created`.
    pub acceptance_texts: BTreeMap<String, String>,
}

/// Cross-studio digest report (`backlot digest --format json`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DigestReport {
    pub schema_version: u32,
    pub since_ms: u64,
    pub now_ms: u64,
    pub unknown_events_skipped: usize,
    pub studios: Vec<StudioDigest>,
}

/// One studio's digest and coverage matrix.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct StudioDigest {
    pub studio: String,
    pub unknown_events: usize,
    /// Declared check names, empty when the studio runs with no checks (`checks: none`).
    pub checks: Vec<String>,
    pub checks_status: String,
    pub studio_finished_ts_ms: Option<u64>,
    pub finished: Vec<FinishItem>,
    pub delivered: Vec<DeliveredItem>,
    pub sent_back: Vec<ReturnedItem>,
    pub blocked: Vec<BlockedTaskItem>,
    pub waiting_on_user: WaitingOnUser,
    pub rate_limits: DigestRateLimitState,
    pub coverage: Vec<CoverageRow>,
}

/// Task finished (accepted/cancelled/etc.) within the digest window.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FinishItem {
    pub task: String,
    pub title: String,
    pub status: String,
    pub ts_ms: u64,
}

/// Delivery recorded within the digest window.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeliveredItem {
    pub task: String,
    pub title: String,
    pub delivery: u32,
    pub commits: Option<String>,
    pub ts_ms: u64,
}

/// Review with `verdict = changes_requested` recorded within the digest window.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnedItem {
    pub task: String,
    pub delivery: u32,
    pub reviewer: String,
    pub note: Option<String>,
    pub ts_ms: u64,
}

/// Currently blocked task at `now_ms`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BlockedTaskItem {
    pub task: String,
    pub title: String,
    pub reason: Option<String>,
    pub waits_for: Vec<String>,
}

/// Items currently waiting on the person.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct WaitingOnUser {
    pub attentions: Vec<OpenAttentionItem>,
    pub asks: Vec<OpenAskItem>,
    pub design_waiting: bool,
}

/// Currently open `NeedsYou` attention.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenAttentionItem {
    pub role: String,
    pub reason: String,
    pub detail: String,
    pub ts_ms: u64,
}

/// Currently unanswered `tell user` question.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenAskItem {
    pub role: String,
    pub seq: u64,
    pub ts_ms: u64,
}

/// Current and in-window rate-limit state for a studio.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DigestRateLimitState {
    pub open: Vec<RateLimitOpenItem>,
    pub events_in_window: usize,
    pub windows_in_window: usize,
    pub max_per_window: usize,
}

/// Currently open rate-limit attention on a role.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RateLimitOpenItem {
    pub role: String,
    pub ts_ms: u64,
    pub reset_at_ms: Option<u64>,
}

/// Coverage matrix row: `design section -> task -> commits -> task.checked -> verdict`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoverageRow {
    pub design_section: Option<String>,
    pub task: String,
    pub task_title: String,
    pub commits: Option<String>,
    pub checks: Vec<CoverageCheck>,
    pub checks_summary: String,
    pub verdict: String,
}

/// Per-check status within a coverage matrix row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoverageCheck {
    pub name: String,
    pub status: String,
}

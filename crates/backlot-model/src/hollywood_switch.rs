//! Wire types for Hollywood queue wait diagnostics (§1.3.2) and
//! make-before-break handoff state (§2.4.1).

use serde::{Deserialize, Serialize};

/// Why queued user messages or actionable notices cannot deliver right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueReason {
    Busy,
    Prompt,
    RateLimited,
    OutOfQuota,
    AuthError,
    Starting,
    WaitingQuiet,
    Handoff,
    Down,
}

/// Summary of queued undelivered messages and why they are waiting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueStatus {
    pub count: u32,
    pub reason: QueueReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Live or terminal status of a make-before-break Hollywood agent switch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffSummary {
    pub id: String,
    pub phase: String,
    pub target_runner: String,
    pub target_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_terminal: Option<String>,
    pub started_ms: u64,
    pub timeout_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

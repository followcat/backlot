//! Typed result structs returned by person verb handlers (B1a).
//!
//! Both `plugin.backlot.res` ack payloads and CLI stdout formatters
//! construct and read these structs so the two paths stay aligned.

use serde::{Deserialize, Serialize};

use crate::event::{ActorIdentity, Priority, TaskStateChange};
use crate::hollywood::{CardAction, CardBase};

/// Result of `tell` (studio role message queued).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TellResult {
    pub studio: String,
    pub role: String,
    pub seq: u64,
    pub file: String,
}

/// Result of `tell-hollywood`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TellHollywoodResult {
    pub seq: u64,
    pub to: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marked_seen: Vec<u64>,
}

/// Result of `card-apply`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardApplyResult {
    pub id: String,
    pub seq: u64,
    pub outcome_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale: Option<String>,
    pub summary: String,
}

/// Result of `card-cancel`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardCancelResult {
    pub id: String,
    pub seq: u64,
    pub outcome_seq: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

/// Old -> new actor identity pair for `recast` and `setup` cards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityChange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old: Option<ActorIdentity>,
    pub new: ActorIdentity,
    pub old_label: String,
    pub new_label: String,
}

/// Draft details attached to a `studio-new` card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StudioNewDraft {
    pub studio: String,
    pub title: String,
    pub repo: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_git: bool,
    pub cast_path: String,
    pub brief_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cast_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_error: Option<String>,
}

/// Structured result of `card-show`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardShowResult {
    pub id: String,
    pub seq: u64,
    pub state: String,
    pub action_label: String,
    pub action: CardAction,
    pub base: CardBase,
    pub why: String,
    pub by: String,
    pub ts_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_change: Option<IdentityChange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub studio_new: Option<StudioNewDraft>,
    /// A `cast` card's draft diff against the live cast, when the card
    /// is proposed against a studio that exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cast_diff: Option<crate::cast::CastDiff>,
}

/// Result of `attention-clear`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionClearResult {
    pub studio: String,
    pub target: u64,
    pub seq: u64,
    pub cleared_role: String,
    pub cleared_reason: String,
}

/// Result of `recast`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecastResult {
    pub studio: String,
    pub person: String,
    pub seq: u64,
    pub old: ActorIdentity,
    pub new: ActorIdentity,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

/// Result of `restart` (studio role or `hollywood`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestartResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub studio: Option<String>,
    pub person: String,
    pub seq: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fresh: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_hollywood: bool,
}

/// Result of `close`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseResult {
    pub studio: String,
    pub person: String,
    pub seq: u64,
}

/// Result of `reopen`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReopenResult {
    pub studio: String,
    pub person: String,
    pub seq: u64,
}

/// Result of `task-priority`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskPriorityResult {
    pub studio: String,
    pub task: String,
    pub seq: u64,
    pub level: Priority,
}

/// Result of `task-pause`, `task-resume`, or `task-abandon`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskStateResult {
    pub studio: String,
    pub task: String,
    pub seq: u64,
    pub state: TaskStateChange,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

/// Result of `hollywood-setup`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HollywoodSetupResult {
    pub runner: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_cast: Option<String>,
    pub action: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub restarted: bool,
}

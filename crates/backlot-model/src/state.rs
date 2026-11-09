//! The reduced studio state's types. Folding lives in `reduce`; the
//! `apply`/`replay`/`reduce_task` names are re-exported here so the
//! `state::` path keeps working.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::correct::SeqTarget;
use crate::event::*;

pub use crate::reduce::{apply, replay};
pub use crate::task_reduce::reduce_task;

/// Studio configuration from `studio.created`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StudioConfig {
    pub title: String,
    pub repo: String,
    pub mode: StudioMode,
    pub roles: RoleConfigs,
    pub candidates: Vec<Candidate>,
    pub rework_cap: u32,
}

/// Task lifecycle (ledger.md task-state diagram). `Running` covers the
/// diagram's `cast` edge: casting dispatches the task to the actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Created,
    Running,
    Delivered,
    Accepted,
    PartiallyAccepted,
    Paused,
    Blocked,
    Abandoned,
    AtCap,
    TakenOver,
}

impl TaskStatus {
    /// States a task never leaves under its own events.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskStatus::Accepted
                | TaskStatus::PartiallyAccepted
                | TaskStatus::Abandoned
                | TaskStatus::TakenOver
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReqRecord {
    pub version: u32,
    pub file: String,
    pub summary: String,
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanRecord {
    pub plan_kind: PlanKind,
    pub file: String,
    pub requirement_version: u32,
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CastRecord {
    pub candidate: String,
    pub actor: ActorIdentity,
    pub reason: String,
    /// The display priority this cast set, when it set one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    pub seq: u64,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeliveryRecord {
    pub n: u32,
    pub actor: ActorIdentity,
    pub commits: String,
    pub report: String,
    pub requirement_version: u32,
    pub checks: Vec<Check>,
    pub seq: u64,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewRecord {
    pub delivery: u32,
    pub reviewer: String,
    pub verdict: Verdict,
    pub conclusion: String,
    pub findings: Vec<Finding>,
    pub upstream_changes: Vec<UpstreamChange>,
    pub seq: u64,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateChangeRecord {
    pub state: TaskStateChange,
    pub reason: String,
    pub seq: u64,
    pub ts_ms: u64,
}

/// A `task.finding` record: defects reported outside the review loop,
/// attributed to whoever made `delivery`. A defect id already seen on
/// the task keeps its id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FindingRecord {
    pub delivery: u32,
    /// Role that recorded it (the event's `by`).
    pub by: String,
    pub findings: Vec<Finding>,
    pub seq: u64,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub task_kind: String,
    /// Requirement version the task was dispatched under (its pin).
    pub requirement_version: u32,
    pub acceptance: String,
    pub dependencies: Vec<String>,
    pub status: TaskStatus,
    /// The status `paused` suspended, restored by `resumed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_from: Option<TaskStatus>,
    /// `changes_requested` reviews so far.
    pub rework_rounds: u32,
    /// Display priority (D11): panel order only, never scheduling.
    #[serde(default)]
    pub priority: Priority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cast: Option<CastRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub casts: Vec<CastRecord>,
    #[serde(default)]
    pub deliveries: Vec<DeliveryRecord>,
    #[serde(default)]
    pub reviews: Vec<ReviewRecord>,
    /// Defects reported by `task.finding` events, in log order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub finding_reports: Vec<FindingRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state_changes: Vec<StateChangeRecord>,
    pub created_seq: u64,
    pub created_ts_ms: u64,
    pub last_ts_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_ts_ms: Option<u64>,
}

impl Task {
    /// Unfinished work whose pin is older than `current`.
    pub fn is_behind(&self, current: u32) -> bool {
        !self.status.is_terminal() && self.requirement_version < current
    }

    pub fn review_of(&self, delivery: u32) -> Option<&ReviewRecord> {
        self.reviews.iter().find(|r| r.delivery == delivery)
    }
}

/// Where a recorded role stands: `live` while its terminal should be
/// up, `lost` when it vanished unexpectedly, `closed` after the stage
/// released it on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleLifecycle {
    Live,
    Lost,
    Closed,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RoleState {
    /// True while the role should have a live terminal.
    pub live: bool,
    /// The stage released this role on purpose (`role.closed`); the
    /// person stays listed and resumable — `actor` and `session_id`
    /// are kept. A later `role.started`/`role.resumed` clears it.
    #[serde(default)]
    pub closed: bool,
    /// `role.reopen` asked for this closed person back (R19): the
    /// stage keeps it desired with a resume until `role.release` or
    /// `role.closed` clears the hold. A resumed role stays held — the
    /// hold is what keeps it desired once live.
    #[serde(default)]
    pub held: bool,
    /// When the current lifecycle state began.
    #[serde(default)]
    pub since_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<ActorIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

impl RoleState {
    /// Live, lost or closed — never more than one.
    pub fn lifecycle(&self) -> RoleLifecycle {
        if self.live {
            RoleLifecycle::Live
        } else if self.closed {
            RoleLifecycle::Closed
        } else {
            RoleLifecycle::Lost
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub seq: u64,
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// `plan` or `dispatch` when the command sent the message with a
    /// fixed meaning; unset for `tell`/`ask`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    pub delivered: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportRecord {
    pub seq: u64,
    pub from: String,
    pub to: String,
    pub node: String,
    pub file: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attention {
    pub seq: u64,
    pub role: String,
    pub reason: String,
    pub detail: String,
    pub open: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorrectionRecord {
    pub seq: u64,
    pub target: u64,
    pub reason: String,
    pub set: BTreeMap<String, String>,
}

/// A whole studio folded from its log.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StudioState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub studio: Option<StudioConfig>,
    /// Latest requirement version; 0 before any `req.version`.
    pub requirement_version: u32,
    #[serde(default)]
    pub requirements: Vec<ReqRecord>,
    #[serde(default)]
    pub plans: Vec<PlanRecord>,
    #[serde(default)]
    pub tasks: BTreeMap<String, Task>,
    #[serde(default)]
    pub roles: BTreeMap<String, RoleState>,
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub reports: Vec<ReportRecord>,
    #[serde(default)]
    pub attention: Vec<Attention>,
    #[serde(default)]
    pub corrections: Vec<CorrectionRecord>,
    /// seq → what the event touched, for corrections and links.
    #[serde(default)]
    pub targets: BTreeMap<u64, SeqTarget>,
    pub last_seq: u64,
}

impl StudioState {
    pub fn rework_cap(&self) -> u32 {
        self.studio
            .as_ref()
            .map(|s| s.rework_cap)
            .unwrap_or_else(default_rework_cap)
    }

    pub fn open_attention(&self) -> impl Iterator<Item = &Attention> {
        self.attention.iter().filter(|a| a.open)
    }
}

/// The task an actor role key works on: `actor-<task>-c<n>` →
/// `<task>`, `None` for any other key.
pub fn actor_task(role: &str) -> Option<&str> {
    let (task, n) = role.strip_prefix("actor-")?.rsplit_once("-c")?;
    if task.is_empty() || n.parse::<usize>().is_err() {
        return None;
    }
    Some(task)
}

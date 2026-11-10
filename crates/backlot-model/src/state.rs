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
    /// Seconds a live role's unchanged screen counts as a stall.
    pub stall_limit_s: u32,
    /// Seconds an owed step's owner may idle before the nudge.
    #[serde(default = "default_nudge_after_s")]
    pub nudge_after_s: u32,
    /// Seconds a delivered nudge may go unanswered before escalation.
    #[serde(default = "default_escalate_after_s")]
    pub escalate_after_s: u32,
    /// Environment variable names passed to every stage-run check.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub check_env: Vec<String>,
    /// The checks the stage runs on each delivery, in declared order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<CheckDecl>,
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
    #[serde(default)]
    pub ts_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CastRecord {
    pub candidate: String,
    pub actor: ActorIdentity,
    pub reason: String,
    /// The display priority this cast set, when it set one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    /// Who recorded the cast (`by.role`); a light-path producer cast
    /// routes the task's delivery notice to the producer. Empty on
    /// logs that predate the field.
    #[serde(default)]
    pub by: String,
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

/// A `task.checked` record: one stage run of one declared check on
/// one delivery — kept apart from the actor-reported `checks` the
/// delivery itself carries (S9b part 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckedRecord {
    pub delivery: u32,
    pub name: String,
    /// The exit code; `None` when a signal killed the run or it
    /// never started — either way not a pass.
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "crate::event::is_false")]
    pub timed_out: bool,
    /// The capped log under `checks/` in the studio dir.
    pub output: String,
    pub duration_ms: u64,
    pub seq: u64,
    pub ts_ms: u64,
}

impl CheckedRecord {
    /// A check passes only on a clean exit — a signal or a timeout
    /// never does.
    pub fn passed(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out
    }
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
    /// Role that recorded it (the event's `by`).
    #[serde(default)]
    pub by: String,
    /// The status a `resumed` restored — `Running` when the actor
    /// should carry on. `reduce_task` fills it during replay, so
    /// `None` only ever marks a non-resume change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resumed_to: Option<TaskStatus>,
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
    /// `task.checked` records, in log order — the stage's results,
    /// separate from each delivery's actor-reported `checks`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checked: Vec<CheckedRecord>,
    #[serde(default)]
    pub reviews: Vec<ReviewRecord>,
    /// Defects reported by `task.finding` events, in log order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub finding_reports: Vec<FindingRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state_changes: Vec<StateChangeRecord>,
    pub created_seq: u64,
    pub created_ts_ms: u64,
    /// Who recorded the `task.created` (`by.role`) — the role an
    /// uncast task's cast step is owed to. Empty on logs that
    /// predate the field.
    #[serde(default)]
    pub created_by: String,
    pub last_ts_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_ts_ms: Option<u64>,
    /// The seq of the event that took the task terminal — the key
    /// of the step that follows it. `None` on logs that predate it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_seq: Option<u64>,
}

impl Task {
    /// Unfinished work whose pin is older than `current`.
    pub fn is_behind(&self, current: u32) -> bool {
        !self.status.is_terminal() && self.requirement_version < current
    }

    pub fn review_of(&self, delivery: u32) -> Option<&ReviewRecord> {
        self.reviews.iter().find(|r| r.delivery == delivery)
    }

    /// The stage-check records on `delivery`, in log order.
    pub fn checked_of(&self, delivery: u32) -> impl Iterator<Item = &CheckedRecord> {
        self.checked.iter().filter(move |c| c.delivery == delivery)
    }

    /// Declared checks with no `task.checked` record on `delivery`
    /// yet, in declaration order.
    pub fn checks_missing(&self, delivery: u32, declared: &[CheckDecl]) -> Vec<String> {
        declared
            .iter()
            .filter(|c| !self.checked_of(delivery).any(|r| r.name == c.name))
            .map(|c| c.name.clone())
            .collect()
    }

    /// Declared checks recorded as not passing on `delivery` — a
    /// nonzero exit, a signal or a timeout — in declaration order.
    pub fn checks_failed(&self, delivery: u32, declared: &[CheckDecl]) -> Vec<String> {
        declared
            .iter()
            .filter(|c| {
                self.checked_of(delivery)
                    .any(|r| r.name == c.name && !r.passed())
            })
            .map(|c| c.name.clone())
            .collect()
    }

    /// Who the task's review-side obligations go to: `producer`
    /// when the producer cast it — the light path has no director —
    /// and the director in every other case.
    pub fn reviewer(&self) -> &'static str {
        match self.cast.as_ref().map(|c| c.by.as_str()) {
            Some("producer") => "producer",
            _ => "director",
        }
    }

    /// Dependencies still open (S9e): not `accepted` or
    /// `partially-accepted` — a name no task carries counts as open
    /// too, so a bad correction keeps the gate shut. The cast
    /// refusal and the `waits for` status marker read the same list.
    pub fn open_dependencies<'s>(&'s self, state: &'s StudioState) -> Vec<&'s str> {
        self.dependencies
            .iter()
            .filter(|d| {
                !matches!(
                    state.tasks.get(d.as_str()).map(|t| t.status),
                    Some(TaskStatus::Accepted | TaskStatus::PartiallyAccepted)
                )
            })
            .map(String::as_str)
            .collect()
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
    /// Every `role.lost` timestamp — the resume guard's crash-loop
    /// history, kept so a stage restart does not lose the count.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub losses: Vec<u64>,
    /// The seq of the `role.resumed` with reason `recovered`, while
    /// the recovered notice is owed — any other resume or start
    /// clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovered_seq: Option<u64>,
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
    /// `plan`, `dispatch` or `nudge` when the message carries a fixed
    /// meaning; unset for `tell`/`ask`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    /// For a stage-generated notice, the seq of the source event it
    /// relays — the dedup key that keeps a restart from repeating it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_seq: Option<u64>,
    pub delivered: bool,
    /// `message.dropped` landed: a queued notice skipped at send time
    /// because its source no longer stood — never pending again.
    #[serde(default, skip_serializing_if = "crate::event::is_false")]
    pub dropped: bool,
    /// The `message.queued` event's time — the obligation it can
    /// create owes from then.
    #[serde(default)]
    pub ts_ms: u64,
    /// When the `message.delivered` landed — a nudge's escalation
    /// clock starts there. `None` on logs that predate it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportRecord {
    pub seq: u64,
    pub from: String,
    pub to: String,
    pub node: String,
    pub file: String,
    /// `message.delivered` on this report's seq — reports to a role
    /// deliver like messages (`to: user` is never sent anywhere).
    #[serde(default)]
    pub delivered: bool,
    /// The `report.sent` event's time.
    #[serde(default)]
    pub ts_ms: u64,
    /// When the `message.delivered` on this report landed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attention {
    pub seq: u64,
    pub role: String,
    pub reason: String,
    pub detail: String,
    pub open: bool,
    /// When the raise was recorded; a permission-prompt episode
    /// dedups on "an attention at or after the episode's start".
    #[serde(default)]
    pub ts_ms: u64,
    /// A `permission prompt` the director may not answer (S9b): the
    /// saved screen reached for the `BACKLOT_*` identity variables
    /// or environment-clearing env flags. Old records carry no
    /// field.
    #[serde(default, skip_serializing_if = "crate::event::is_false")]
    pub needs_user: bool,
}

/// A recorded `role.approval` — a permission-prompt decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub seq: u64,
    pub role: String,
    /// Who recorded the decision — the deny notice names them.
    /// Older ledgers have no `by`.
    #[serde(default)]
    pub by: String,
    pub decision: ApprovalDecision,
    pub ts_ms: u64,
}

/// The `studio.finished` record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FinishRecord {
    pub seq: u64,
    pub reason: String,
    pub ts_ms: u64,
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
    /// `role.approval` records, in log order.
    #[serde(default)]
    pub approvals: Vec<ApprovalRecord>,
    /// `studio.finished`; `Some` means the studio ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished: Option<FinishRecord>,
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

    pub fn stall_limit_s(&self) -> u32 {
        self.studio
            .as_ref()
            .map(|s| s.stall_limit_s)
            .unwrap_or_else(default_stall_limit_s)
    }

    pub fn nudge_after_s(&self) -> u32 {
        self.studio
            .as_ref()
            .map(|s| s.nudge_after_s)
            .unwrap_or_else(default_nudge_after_s)
    }

    pub fn escalate_after_s(&self) -> u32 {
        self.studio
            .as_ref()
            .map(|s| s.escalate_after_s)
            .unwrap_or_else(default_escalate_after_s)
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

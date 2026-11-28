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
    /// The bound path (D4): the repository itself, the workspace
    /// directory, or the plain directory.
    pub repo: String,
    /// Workspace member repositories, as absolute paths — empty unless
    /// the bound path is a workspace (D4, D10).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
    /// Bound with no git at all: every role shares `repo`, no
    /// worktree isolation (D4, D10).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_git: bool,
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
    /// The optional per-task running budget in seconds (S9e).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_budget_s: Option<u32>,
    /// Runner name → maximum live roles in this studio (S9e).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub limits: BTreeMap<String, u32>,
    /// At most this many actors live at once; fixed roles never count.
    #[serde(default = "default_max_actors")]
    pub max_actors: u32,
    /// Studio outward push policy and recorded remote URLs.
    #[serde(default, skip_serializing_if = "PushPolicy::is_default")]
    pub push: PushPolicy,
    /// Opening design gate policy; absent on old logs reads as `Off`.
    #[serde(default, skip_serializing_if = "DesignGate::is_off")]
    pub design_gate: DesignGate,
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
    /// Runner-reported token/cost figures (P4a); absent when the
    /// delivery carried none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_failed: Option<String>,
    /// The capped log under `checks/` in the studio dir.
    pub output: String,
    pub duration_ms: u64,
    pub seq: u64,
    pub ts_ms: u64,
}

impl CheckedRecord {
    /// A check passes only on a clean exit with no timeout and no failed `expect`.
    pub fn passed(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out && self.expect_failed.is_none()
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
    /// The task's first actor `role.started` — the budget clock's
    /// origin (S9e): a cast that waits on a runner cap accrues
    /// nothing before it, and a recast does not rewind it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_started_ms: Option<u64>,
}

impl Task {
    /// Whether this task is a read-only audit (`audit` kind or `readonly` cast).
    pub fn is_readonly_or_audit(&self) -> bool {
        self.task_kind == "audit"
            || self
                .cast
                .as_ref()
                .is_some_and(|c| c.actor.permission_profile == "readonly")
    }

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

/// A `role.restart` the stage still owes (L2): stop the role's
/// terminal, then launch it again under the same identity — resuming
/// the recorded session, or `fresh` starting a new one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RestartRequest {
    /// The `role.restart` event's seq.
    pub seq: u64,
    /// Start a new session instead of resuming the recorded one.
    #[serde(default, skip_serializing_if = "crate::event::is_false")]
    pub fresh: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
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
    /// When the stage last answered a dialog on this role's screen
    /// (`stage.answered`) — the drawer's `trust answered by stage`
    /// line. Bookkeeping only: no attention, nothing to clear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_answer_ms: Option<u64>,
    /// A `role.restart` the stage still owes this role — cleared by
    /// the `role.closed` (reason `restarted`) that carries it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<RestartRequest>,
    /// The newest time the stage restarted the role in place — the
    /// `restarted` word the summary and Hollywood show.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restarted_ms: Option<u64>,
    /// The role's last one-line progress note from the ledger (`role.progress`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<crate::progress::RoleProgress>,
    /// Live deepest descendant process (`status --json` overlay only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running: Option<crate::progress::RoleRunning>,
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
    /// `message.dropped` landed: a queued message skipped at send time
    /// — never pending again.
    #[serde(default, skip_serializing_if = "crate::event::is_false")]
    pub dropped: bool,
    /// Reason recorded by `message.dropped`, omitted until dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped_reason: Option<String>,
    /// The `message.queued` event's time — the obligation it can
    /// create owes from then.
    #[serde(default)]
    pub ts_ms: u64,
    /// `agent:<name>` when a coding agent wrote it for the person (the
    /// `BACKLOT_VIA`/`--via` record); `"doc"` for the server queue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
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
    /// The `attention.cleared` reason once closed — `sent <key>`
    /// marks a bound permission request as answered (S9f), so the
    /// stage can rebuild that set from the ledger alone. Old
    /// records carry no field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleared_reason: Option<String>,
    /// Implicit/resolved quota account when `reason == "rate limited"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// Unclamped reset epoch ms when known on `reason == "rate limited"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_at_ms: Option<u64>,
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

/// The `studio.reopened` record — the last time the person brought a
/// finished studio back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReopenRecord {
    pub seq: u64,
    pub reason: String,
    pub ts_ms: u64,
    /// The `c<n>` marker a card's execution carries — the gate's
    /// "already done" key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorrectionRecord {
    pub seq: u64,
    pub target: u64,
    pub reason: String,
    pub set: BTreeMap<String, String>,
}

/// A `role.recast` record: the newest identity a fixed role plays
/// from `seq` on. Earlier deliveries and reviews keep the identity
/// they were recorded with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecastRecord {
    pub role: String,
    pub actor: ActorIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The `c<n>` marker a card's execution carries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<String>,
    pub seq: u64,
    pub ts_ms: u64,
}

/// A `role.restart` record, in log order — the card gate's
/// "already done" marker reads the reason it carried.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RestartRecord {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub seq: u64,
    pub ts_ms: u64,
}

/// An `ask.answered` record: the stage saw `role` start a new turn in
/// its terminal after the `tell user` at `target`, clearing the
/// waiting indicator while keeping the question in the exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskAnswerRecord {
    pub seq: u64,
    pub role: String,
    pub target: u64,
    pub ts_ms: u64,
}

/// The newest ledger event written by a role (`event.by.role`), used
/// as ledger liveness evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleLastEvent {
    pub seq: u64,
    pub ts_ms: u64,
    pub kind: String,
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
    /// `role.recast` records, in log order — the newest identity for
    /// a fixed role wins.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recasts: Vec<RecastRecord>,
    /// `role.restart` records, in log order — the card gate's
    /// idempotence marker.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restarts: Vec<RestartRecord>,
    /// `ask.answered` records, in log order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ask_answers: Vec<AskAnswerRecord>,
    /// Role key → highest seq where the stage sent into that role's
    /// terminal or changed its lifecycle since an ask.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub role_stage_touch_seq: BTreeMap<String, u64>,
    /// `studio.finished`; `Some` means the studio ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished: Option<FinishRecord>,
    /// The newest `studio.reopened` — `Some` with `finished` `None`
    /// while the studio runs again. A finish newer than this is the
    /// state that counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reopened: Option<ReopenRecord>,
    #[serde(default)]
    pub corrections: Vec<CorrectionRecord>,
    /// The seq of the newest `studio.cast` — the settings event a
    /// `cast` card bases on, so a change in between makes it stale.
    /// `None` on a studio whose cast has never changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cast_seq: Option<u64>,
    /// The `c<n>` marker on the newest `studio.cast`, when a card
    /// carried the change out — the gate's "already done" key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cast_card: Option<String>,
    /// seq → what the event touched, for corrections and links.
    #[serde(default)]
    pub targets: BTreeMap<u64, SeqTarget>,
    /// Token/cost totals summed over every delivery's `usage` (P4a)
    /// — strictly factual, never a score (D8). Absent while zero so
    /// states with no usage serialize exactly as before.
    #[serde(default, skip_serializing_if = "UsageTotals::is_empty")]
    pub usage: UsageTotals,
    /// `by.role` → the newest event written by that role.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub last_by_role: BTreeMap<String, RoleLastEvent>,
    /// `by.role` → the newest event by that role that is not a bare
    /// `role.progress` note. The nudge pass reads it for an actor's
    /// delivery step, where progress is not activity. Skipped on
    /// serialization: replay rebuilds it.
    #[serde(default, skip_serializing)]
    pub last_act_by_role: BTreeMap<String, RoleLastEvent>,
    /// Role key → latest `role.progress` note (even before `role.started`);
    /// skipped on serialization because `status --json` publishes `progress` once per role.
    #[serde(default, skip_serializing)]
    pub progress: BTreeMap<String, crate::progress::RoleProgress>,
    /// Carded outward requests (`role.refused` with `disposition: Card`, or
    /// `outward.requested`) recorded in seq order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outward_requests: Vec<OutwardRequestRecord>,
    /// Completed/failed/denied/stale outward outcomes recorded by `stage`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outward_completions: Vec<OutwardCompletedRecord>,
    /// `EventKind::Unknown` kind names encountered during replay.
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub unknown_kinds: std::collections::BTreeSet<String>,
    /// Active opening design discussion state for the current requirement version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design: Option<crate::design::DesignState>,
    /// Signed design records retained from prior requirement versions.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub design_history: BTreeMap<u32, crate::design::DesignState>,
    /// Active line stop from `studio.halted`, cleared by `studio.resumed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub halted: Option<HaltRecord>,
    /// Chronological halt/resume episodes for metrics and audit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub halts: Vec<HaltEpisode>,
    /// Non-probe and probe `role.refused` records rebuilt on replay.
    #[serde(skip)]
    pub role_refusals: Vec<RoleRefusalRecord>,
    /// `attention.seq` -> clear timestamp (`ts_ms`) rebuilt on replay.
    #[serde(skip)]
    pub attention_cleared_ms: BTreeMap<u64, u64>,
    pub last_seq: u64,
}

/// Active explicit line stop (`studio.halted`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HaltRecord {
    pub seq: u64,
    pub by: String,
    pub reason: String,
    pub since_ms: u64,
}

/// One halt/resume cycle stored on [`StudioState::halts`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HaltEpisode {
    pub halted_seq: u64,
    pub by: String,
    pub reason: String,
    pub since_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resumed_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resumed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resumed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "crate::event::is_false")]
    pub false_positive: bool,
}

/// Compact reduced record for one `role.refused` ledger event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleRefusalRecord {
    pub seq: u64,
    pub ts_ms: u64,
    pub role: String,
    pub command: String,
    pub rule: Option<String>,
    pub disposition: RefusalDisposition,
    pub is_probe: bool,
}

/// One carded outward command request (`role.refused` with `Card` or `outward.requested`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutwardRequestRecord {
    pub seq: u64,
    pub ts_ms: u64,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    pub command: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub argv: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
}

/// One `outward.completed` record in the studio ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutwardCompletedRecord {
    pub seq: u64,
    pub ts_ms: u64,
    pub card: u64,
    pub source_seq: u64,
    pub outcome: OutwardOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub output_tail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_file: Option<String>,
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

    pub fn has_outward_completion(&self, card: u64) -> bool {
        self.outward_completions
            .iter()
            .any(|c| c.card != 0 && c.card == card)
    }

    pub fn has_outward_source_completed(&self, source_seq: u64) -> bool {
        self.outward_completions
            .iter()
            .any(|c| c.source_seq == source_seq)
    }

    /// Current opening design phase for [`Self::requirement_version`].
    pub fn design_phase(&self) -> crate::design::DesignPhase {
        self.design
            .as_ref()
            .filter(|d| d.version == self.requirement_version)
            .map(|d| d.phase)
            .unwrap_or(crate::design::DesignPhase::Idle)
    }

    /// The identity a fixed role plays now: the newest `role.recast`
    /// for it, else the `studio.created` cast. `None` for an actor
    /// key — an actor's identity lives on its task's newest cast
    /// record (`Task.cast`), which a recast of the actor key rewrites.
    pub fn fixed_identity(&self, role: &str) -> Option<&ActorIdentity> {
        let cfg = self.studio.as_ref()?;
        if !matches!(role, "producer" | "director" | "writer") {
            return None;
        }
        self.recasts
            .iter()
            .rev()
            .find(|r| r.role == role)
            .map(|r| &r.actor)
            .or(match role {
                "producer" => Some(&cfg.roles.producer),
                "director" => Some(&cfg.roles.director),
                _ => Some(&cfg.roles.writer),
            })
    }

    /// Current effective identity of any studio role (`producer`, `director`,
    /// `writer`, or `actor-<task>-c<n>`).
    pub fn role_actor(&self, role: &str) -> Option<ActorIdentity> {
        if matches!(role, "producer" | "director" | "writer") {
            return self.fixed_identity(role).cloned();
        }
        if let Some(task_id) = actor_task(role)
            && let Some(task) = self.tasks.get(task_id)
            && let Some((_, num)) = role.rsplit_once("-c")
            && let Ok(n) = num.parse::<usize>()
            && n >= 1
        {
            if task.casts.len() == n
                && let Some(c) = &task.cast
            {
                return Some(c.actor.clone());
            }
            if let Some(c) = task.casts.get(n - 1) {
                return Some(c.actor.clone());
            }
        }
        self.roles.get(role).and_then(|r| r.actor.clone())
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

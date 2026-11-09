//! Ledger event types, per `docs/studio/ledger.md`. Events are the
//! source of truth; every event has `seq`, `ts_ms`, `by` and `kind`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Who recorded the event: the role and, when known, the hc terminal id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventBy {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

impl EventBy {
    pub fn role(role: impl Into<String>) -> Self {
        EventBy {
            role: role.into(),
            session: None,
        }
    }
}

/// The complete configuration that did the work (ledger.md "Actor
/// identity"). `runner_version` is optional until runner adapters land.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ActorIdentity {
    pub runner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_version: Option<String>,
    pub model: String,
    pub effort: String,
    pub permission_profile: String,
}

/// A named actor configuration the director may cast from (R11).
/// Kept flat (not `#[serde(flatten)]`) so it also parses from TOML.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub name: String,
    pub runner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_version: Option<String>,
    pub model: String,
    pub effort: String,
    pub permission_profile: String,
}

impl Candidate {
    pub fn actor(&self) -> ActorIdentity {
        ActorIdentity {
            runner: self.runner.clone(),
            runner_version: self.runner_version.clone(),
            model: self.model.clone(),
            effort: self.effort.clone(),
            permission_profile: self.permission_profile.clone(),
        }
    }
}

/// Configured identities of the long-lived roles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleConfigs {
    pub producer: ActorIdentity,
    pub director: ActorIdentity,
    pub writer: ActorIdentity,
}

/// Studio process mode; experimental production is a later milestone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudioMode {
    #[default]
    Standard,
    Experimental,
}

/// What the writer publishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanKind {
    Plan,
    Acceptance,
}

/// A recorded `role.approval` decision answering an open `permission
/// prompt` attention (D9). The stage sends the runner's approval key
/// only while the prompt still shows — never without one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Allow,
    Deny,
}

/// The `attention` reason a permission prompt raises (D9): a role
/// sampled `blocked` for two samples raises one per blocked episode.
pub const PERMISSION_PROMPT: &str = "permission prompt";

/// The `attention` reason a stalled role raises (S6): a live role's
/// screen unchanged for the studio's `stall_limit_s`.
pub const STALLED: &str = "stalled";

/// The `attention` reason for a lost role the stage cannot bring
/// back (S6b): no recorded session id, or too many losses inside
/// the guard window.
pub const CANNOT_RESUME: &str = "cannot resume";

/// `role.resumed` reason for a held person `role.reopen` asked back.
pub const REOPENED: &str = "reopened";

/// `role.resumed` reason for a lost role the stage resumed after a
/// restart — and the notice label that tells it so.
pub const RECOVERED: &str = "recovered";

/// A task's display priority (R20, D11): it orders people in the
/// panel and nothing else — scheduling never reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Priority::Low => "low",
            Priority::Normal => "normal",
            Priority::High => "high",
        };
        f.write_str(s)
    }
}

impl std::str::FromStr for Priority {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "low" => Ok(Priority::Low),
            "normal" => Ok(Priority::Normal),
            "high" => Ok(Priority::High),
            _ => Err(()),
        }
    }
}

/// Review verdict (ledger.md task-state diagram).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Accepted,
    PartiallyAccepted,
    ChangesRequested,
    Blocked,
}

/// Explicit task state changes carried by `task.state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStateChange {
    Paused,
    Resumed,
    Abandoned,
    TakenOver,
    /// Recorded by `backlot block`: the actor cannot proceed.
    Blocked,
}

/// A check the actor ran before delivering (`--check name=pass|fail`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub passed: bool,
}

/// Finding severity; the ledger names the field but not the values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Low,
    Medium,
    High,
}

/// Why a defect happened (ledger.md cause table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    DeliveryDefect,
    UnresolvedRework,
    RegressionFromFix,
    RequirementChange,
    UpstreamError,
    Environment,
    ResearchNegative,
    /// The default until there is evidence.
    #[default]
    Unattributed,
}

impl Cause {
    /// Causes counted as the actor's own failure in the record view.
    pub fn is_actor_fault(self) -> bool {
        matches!(
            self,
            Cause::DeliveryDefect | Cause::UnresolvedRework | Cause::RegressionFromFix
        )
    }
}

/// One defect record, stable across rework rounds via `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub summary: String,
    pub severity: Severity,
    #[serde(default)]
    pub cause: Cause,
}

/// A fix the director made itself on a delivery. `defect` links it to
/// a defect only when the delivery really had one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpstreamChange {
    pub summary: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defect: Option<String>,
}

/// The event body. `kind` is the serde tag, so payload fields that the
/// ledger calls "kind" are spelled `plan_kind` / `task_kind`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum EventKind {
    #[serde(rename = "studio.created")]
    StudioCreated {
        title: String,
        repo: String,
        #[serde(default)]
        mode: StudioMode,
        roles: Box<RoleConfigs>,
        candidates: Vec<Candidate>,
        #[serde(default = "default_rework_cap")]
        rework_cap: u32,
        /// Seconds a live role's unchanged screen counts as a stall.
        #[serde(default = "default_stall_limit_s")]
        stall_limit_s: u32,
    },
    #[serde(rename = "role.started")]
    RoleStarted {
        role: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        terminal: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        actor: Option<ActorIdentity>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    #[serde(rename = "role.lost")]
    RoleLost {
        role: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    #[serde(rename = "role.resumed")]
    RoleResumed {
        role: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        terminal: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        actor: Option<ActorIdentity>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// The stage released the role on purpose (no longer desired, a
    /// recast replacement, an orphan). Distinct from `role.lost`: the
    /// role did not vanish unexpectedly, so the reduced state keeps
    /// `actor` and `session_id` — a closed role stays resumable. A
    /// later `role.started` or `role.resumed` clears the closed state.
    #[serde(rename = "role.closed")]
    RoleClosed {
        role: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// The session id a runner reports after launch, found by the
    /// stage once the role is live (`role.started` already carries
    /// the ids adapters choose at launch).
    #[serde(rename = "role.session")]
    RoleSession { role: String, session_id: String },
    /// Ask to reopen a closed person (R19): sets `held` so the stage
    /// wants it back, resumed in the session it closed with. Refused
    /// unless the role is closed, has a session id and is not already
    /// held — and while another live role uses its terminal name.
    #[serde(rename = "role.reopen")]
    RoleReopen { role: String },
    /// Let a held person go: `held` clears, the role stops being
    /// desired and the existing Stop path closes it. Refused unless
    /// the role is held.
    #[serde(rename = "role.release")]
    RoleRelease { role: String },
    #[serde(rename = "req.version")]
    ReqVersion {
        version: u32,
        file: String,
        summary: String,
    },
    #[serde(rename = "plan.published")]
    PlanPublished {
        plan_kind: PlanKind,
        file: String,
        requirement_version: u32,
    },
    #[serde(rename = "task.created")]
    TaskCreated {
        task: String,
        title: String,
        task_kind: String,
        requirement_version: u32,
        acceptance: String,
        #[serde(default)]
        dependencies: Vec<String>,
    },
    #[serde(rename = "task.cast")]
    TaskCast {
        task: String,
        candidate: String,
        actor: ActorIdentity,
        #[serde(default)]
        reason: String,
        /// Display priority at cast time; absent keeps the task's
        /// current one. Old logs carry no field and still load.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        priority: Option<Priority>,
    },
    /// A display-priority change (D11): order only, never
    /// scheduling. The task must exist — open or closed.
    #[serde(rename = "task.priority")]
    TaskPriority { task: String, priority: Priority },
    #[serde(rename = "task.delivered")]
    TaskDelivered {
        task: String,
        delivery: u32,
        actor: ActorIdentity,
        commits: String,
        report: String,
        requirement_version: u32,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        checks: Vec<Check>,
    },
    #[serde(rename = "task.reviewed")]
    TaskReviewed {
        task: String,
        delivery: u32,
        reviewer: String,
        verdict: Verdict,
        conclusion: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        findings: Vec<Finding>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        upstream_changes: Vec<UpstreamChange>,
    },
    /// A defect found outside the review loop — integration, real
    /// use — attributed to whoever made `delivery`. Never changes the
    /// task's status. `by` on the envelope is who recorded it.
    #[serde(rename = "task.finding")]
    TaskFinding {
        task: String,
        delivery: u32,
        findings: Vec<Finding>,
    },
    #[serde(rename = "task.state")]
    TaskState {
        task: String,
        state: TaskStateChange,
        #[serde(default)]
        reason: String,
    },
    #[serde(rename = "message.queued")]
    MessageQueued {
        to: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
        /// What the message is for, when a command sends one with a
        /// fixed meaning: `plan` for `plan request`, `dispatch` for
        /// `dispatch`. Plain `tell`/`ask` leave it unset — the stage
        /// reads it to know when the writer is wanted.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        purpose: Option<String>,
        /// The seq of the source event a stage-generated notice
        /// relays, so a notice is owed only while no message carries
        /// its source's seq — a stage restart repeats nothing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_seq: Option<u64>,
    },
    /// `target` is the seq of the `message.queued` event.
    #[serde(rename = "message.delivered")]
    MessageDelivered { target: u64 },
    #[serde(rename = "report.sent")]
    ReportSent {
        to: String,
        node: String,
        file: String,
    },
    #[serde(rename = "attention.raised")]
    AttentionRaised {
        role: String,
        reason: String,
        #[serde(default)]
        detail: String,
    },
    /// `target` is the seq of the `attention.raised` event.
    #[serde(rename = "attention.cleared")]
    AttentionCleared {
        target: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// `role.approval`: a permission-prompt decision (D9). Refused
    /// unless `role` has an open `permission prompt` attention.
    #[serde(rename = "role.approval")]
    RoleApproval {
        role: String,
        decision: ApprovalDecision,
    },
    /// `studio.finished`: the producer or user ended the studio.
    /// Refused while any task is not terminal. From here on only
    /// explicitly reopened people are desired.
    #[serde(rename = "studio.finished")]
    StudioFinished {
        #[serde(default)]
        reason: String,
    },
    /// `target` is the seq of the event being corrected; `set` holds
    /// the changed fields. The earlier event stays in the log.
    #[serde(rename = "correction")]
    Correction {
        target: u64,
        reason: String,
        set: BTreeMap<String, String>,
    },
}

/// Default rework cap per task (design.md "Stop conditions").
pub fn default_rework_cap() -> u32 {
    2
}

/// Default stall limit in seconds (design.md "Watching agents").
pub fn default_stall_limit_s() -> u32 {
    600
}

impl EventKind {
    /// The wire name, identical to the serde tag.
    pub fn name(&self) -> &'static str {
        match self {
            EventKind::StudioCreated { .. } => "studio.created",
            EventKind::RoleStarted { .. } => "role.started",
            EventKind::RoleLost { .. } => "role.lost",
            EventKind::RoleResumed { .. } => "role.resumed",
            EventKind::RoleClosed { .. } => "role.closed",
            EventKind::RoleSession { .. } => "role.session",
            EventKind::RoleReopen { .. } => "role.reopen",
            EventKind::RoleRelease { .. } => "role.release",
            EventKind::ReqVersion { .. } => "req.version",
            EventKind::PlanPublished { .. } => "plan.published",
            EventKind::TaskCreated { .. } => "task.created",
            EventKind::TaskCast { .. } => "task.cast",
            EventKind::TaskPriority { .. } => "task.priority",
            EventKind::TaskDelivered { .. } => "task.delivered",
            EventKind::TaskReviewed { .. } => "task.reviewed",
            EventKind::TaskFinding { .. } => "task.finding",
            EventKind::TaskState { .. } => "task.state",
            EventKind::MessageQueued { .. } => "message.queued",
            EventKind::MessageDelivered { .. } => "message.delivered",
            EventKind::ReportSent { .. } => "report.sent",
            EventKind::AttentionRaised { .. } => "attention.raised",
            EventKind::AttentionCleared { .. } => "attention.cleared",
            EventKind::RoleApproval { .. } => "role.approval",
            EventKind::StudioFinished { .. } => "studio.finished",
            EventKind::Correction { .. } => "correction",
        }
    }
}

/// An event as appended to the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    pub ts_ms: u64,
    pub by: EventBy,
    #[serde(flatten)]
    pub kind: EventKind,
}

/// An event before sequencing: the store assigns `seq` and `ts_ms`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewEvent {
    pub by: EventBy,
    #[serde(flatten)]
    pub kind: EventKind,
}

impl NewEvent {
    pub fn new(by: EventBy, kind: EventKind) -> Self {
        NewEvent { by, kind }
    }

    /// Stamp with the seq assigned under the append lock.
    pub fn stamp(self, seq: u64, ts_ms: u64) -> Event {
        Event {
            seq,
            ts_ms,
            by: self.by,
            kind: self.kind,
        }
    }
}

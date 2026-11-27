//! Ledger event types, per `docs/studio/ledger.md`. Events are the
//! source of truth; every event has `seq`, `ts_ms`, `by` and `kind`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use crate::design::{DesignGate, DesignQuestion, DesignSlot, EntrySource, SignMode};
pub use crate::progress::{
    MAX_PROGRESS_CHARS, MAX_RUNNING_CHARS, age_short, normalize_progress_text,
};
pub use crate::push_policy::{PushPolicy, RecordedRemote};
/// `Usage`/`UsageTotals` live in `crate::usage`; the `event::`
/// path keeps working for the event payloads that carry them.
pub use crate::usage::{Usage, UsageTotals};

/// Who recorded the event: the role and, when known, the hc terminal id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventBy {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// Additive transport origin (`"doc"` when submitted through `plugin.backlot.req`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
}

impl EventBy {
    pub fn role(role: impl Into<String>) -> Self {
        EventBy {
            role: role.into(),
            session: None,
            via: None,
        }
    }

    pub fn doc(role: impl Into<String>) -> Self {
        EventBy {
            role: role.into(),
            session: None,
            via: Some("doc".into()),
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
    /// Thinking depth — absent for a runner that carries it in the
    /// model id or takes no flag (`cursor`, `devin`, `cmd`,
    /// `opencode`); never written when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
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
    /// Thinking depth — absent for a runner that carries it in the
    /// model id or takes no flag (`cursor`, `devin`, `cmd`,
    /// `opencode`); never written when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
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

/// Disposition of a `role.refused` guard verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalDisposition {
    #[default]
    Refused,
    Card,
}

impl RefusalDisposition {
    pub(crate) fn is_default(d: &Self) -> bool {
        *d == Self::Refused
    }
}

/// Terminal outcome recorded in `outward.completed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutwardOutcome {
    Succeeded,
    Failed,
    TimedOut,
    Denied,
    Stale,
}

/// The `attention` reason a permission prompt raises (D9): a role
/// sampled `blocked` for two samples raises one per blocked episode.
pub const PERMISSION_PROMPT: &str = "permission prompt";

/// The `attention` reason an interactive question or unrecognised
/// dialog picker raises (R7b, D2): an actor's routes to its task
/// reviewer, the director's and writer's to the producer, and the
/// producer's surfaces `needs_user` for the user.
pub const QUESTION: &str = "question";

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

/// The `role.closed` reason the stage records when it stops a role to
/// restart it, and the `role.started`/`role.resumed` reason of the
/// start that follows: one deliberate stop, never counted as a loss.
pub const RESTARTED: &str = "restarted";

/// `message.dropped` reason when queued mail targets an actor whose
/// session has closed for good (finished, abandoned, closed, or
/// replaced) with no resume and no live terminal for that actor id.
pub const ACTOR_CLOSED: &str = "actor_closed";

/// The `dialog` a `stage.answered` names today: Claude Code's
/// first-use folder-trust dialog.
pub const FOLDER_TRUST: &str = "folder-trust";

/// The `attention` reason an outward-action guard refusal raises
/// (D3): a shim or git hook stopped an outward command, so the user
/// decides whether to run it outside backlot.
pub const OUTWARD_ACTION: &str = "outward action";

/// A task's display priority (R20, D11): it orders people in the
/// panel — and, since S9e, the actors a runner cap picks first.
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

/// A check the cast declares for the stage to run on each delivery
/// (`[[checks]]`, S9b part 2). The stage runs `run` under a stripped
/// environment in a worktree of the delivery's tip and records
/// `task.checked`; `timeout_s` bounds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckDecl {
    pub name: String,
    pub run: String,
    pub timeout_s: u64,
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
        /// The bound path (D4): repository, workspace directory or
        /// plain directory.
        repo: String,
        /// Workspace member repositories, as absolute paths — empty
        /// unless the bound path is a workspace.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        members: Vec<String>,
        /// Bound with no git at all: every role shares `repo`.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        no_git: bool,
        #[serde(default)]
        mode: StudioMode,
        roles: Box<RoleConfigs>,
        candidates: Vec<Candidate>,
        #[serde(default = "default_rework_cap")]
        rework_cap: u32,
        /// Seconds a live role's unchanged screen counts as a stall.
        #[serde(default = "default_stall_limit_s")]
        stall_limit_s: u32,
        /// Seconds an owed step's owner may idle before the nudge.
        #[serde(default = "default_nudge_after_s")]
        nudge_after_s: u32,
        /// Seconds a delivered nudge may go unanswered, still idle,
        /// before the `owed` attention.
        #[serde(default = "default_escalate_after_s")]
        escalate_after_s: u32,
        /// Environment variable names passed through to every
        /// stage-run check (`check_env` in the cast file).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        check_env: Vec<String>,
        /// The checks the stage runs on each delivery, in declared
        /// order. Absent on logs that predate the field.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        checks: Vec<CheckDecl>,
        /// The optional per-task running budget in seconds (S9e): a
        /// running or delivered task past it raises one `over
        /// budget` attention. Absent means no budget.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_budget_s: Option<u32>,
        /// The cast's `[limits]` table (S9e): runner name → maximum
        /// live roles in this studio; a missing runner is unlimited.
        /// Absent on logs that predate the field.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        limits: BTreeMap<String, u32>,
        /// The studio-wide actor cap: at most this many actors live at
        /// once, fixed roles never counting. Absent on logs that
        /// predate the field reads as [`default_max_actors`].
        #[serde(default = "default_max_actors")]
        max_actors: u32,
        /// Studio outward push policy and recorded remote URLs.
        #[serde(default, skip_serializing_if = "PushPolicy::is_default")]
        push: PushPolicy,
        /// Opening design gate policy; absent on old logs reads as `Off`.
        #[serde(default, skip_serializing_if = "DesignGate::is_off")]
        design_gate: DesignGate,
    },
    /// `studio.cast`: the studio's cast changed after creation — the
    /// person's own `studio cast set`, or a card the stage carries out
    /// as the user. It carries the full new settings; the reducer uses
    /// the newest `studio.cast` (or `studio.created`) from then on and
    /// earlier events keep what they recorded. The binding — `repo`,
    /// `members`, `no_git`, `mode` — is not cast and never travels
    /// here; `push.remotes` are facts recorded at creation and are
    /// kept.
    #[serde(rename = "studio.cast")]
    StudioCast {
        roles: Box<RoleConfigs>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        candidates: Vec<Candidate>,
        #[serde(default = "default_rework_cap")]
        rework_cap: u32,
        #[serde(default = "default_stall_limit_s")]
        stall_limit_s: u32,
        #[serde(default = "default_nudge_after_s")]
        nudge_after_s: u32,
        #[serde(default = "default_escalate_after_s")]
        escalate_after_s: u32,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        check_env: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        checks: Vec<CheckDecl>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_budget_s: Option<u32>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        limits: BTreeMap<String, u32>,
        /// The studio-wide actor cap; see [`StudioCreated`]'s.
        #[serde(default = "default_max_actors")]
        max_actors: u32,
        #[serde(default, skip_serializing_if = "PushPolicy::is_default")]
        push: PushPolicy,
        /// Opening design gate policy (`required` or `off`).
        #[serde(default, skip_serializing_if = "DesignGate::is_off")]
        design_gate: DesignGate,
        /// The `c<n>` marker when a card carried the change out — the
        /// idempotence key on a restart, like `role.recast`'s.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        card: Option<String>,
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
    /// Recast a fixed role onto a new identity (the user's, or a
    /// card's carried out by the stage as the user). The reduced
    /// state uses the newest identity for that role from then on;
    /// every earlier event keeps the identity it recorded, so
    /// deliveries and reviews stay attributed to whoever made them.
    #[serde(rename = "role.recast")]
    RoleRecast {
        role: String,
        actor: ActorIdentity,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        /// The `c<n>` marker when a card carried the event out — the
        /// idempotence key on a restart, like `task.state`'s.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        card: Option<String>,
    },
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
    /// Restart a live or lost person in place: the stage stops its
    /// terminal (`role.closed`, reason `restarted`) and launches it
    /// again under the same identity — resuming the recorded session,
    /// or `fresh` starting a new one with the role's prompt. Refused
    /// for a closed person (reopen is that) and a finished studio.
    #[serde(rename = "role.restart")]
    RoleRestart {
        role: String,
        #[serde(default, skip_serializing_if = "is_false")]
        fresh: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
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
        /// Runner-reported token/cost figures (P4a) — absent on logs
        /// that predate the field and on deliveries that report none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
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
    /// `task.checked`: the stage's own run of one declared check on
    /// one delivery — never the actor's say-so (S9b part 2).
    #[serde(rename = "task.checked")]
    TaskChecked {
        task: String,
        /// The delivery it ran on (`task.delivered`'s `delivery`).
        delivery: u32,
        /// The declared check's name.
        name: String,
        /// The exit code; `None` when a signal killed the run or
        /// it never started — either way not a pass.
        exit_code: Option<i32>,
        /// The run hit its `timeout_s` and the stage killed it.
        #[serde(default, skip_serializing_if = "is_false")]
        timed_out: bool,
        /// The capped log under `checks/` in the studio dir.
        output: String,
        duration_ms: u64,
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
    /// `message.dropped` (S9d): a queued notice's source no longer
    /// stands — recorded instead of sent, with `reason` naming the
    /// rule that caught it. `target` is the `message.queued` seq.
    #[serde(rename = "message.dropped")]
    MessageDropped { target: u64, reason: String },
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
        /// A `permission prompt` the director may not answer (S9b):
        /// the stage flagged the saved screen on the `BACKLOT_*`
        /// identity variables or environment-clearing env flags, so
        /// the attention is the user's. Absent on old logs.
        #[serde(default, skip_serializing_if = "is_false")]
        needs_user: bool,
        /// Canonical `<runner>:<label>` account when `reason == "rate limited"`.
        /// Absent on older logs and non-rate-limit attentions.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account: Option<String>,
        /// Unclamped epoch ms when the quota is expected to reset, when known
        /// on `reason == "rate limited"`. Absent on older logs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reset_at_ms: Option<u64>,
    },
    /// `attention.escalated`: the same open attention becomes the
    /// person's — one stall keeps one attention rather than a close
    /// plus a fresh raise. `detail` replaces the display detail.
    #[serde(rename = "attention.escalated")]
    AttentionEscalated { target: u64, detail: String },
    /// `target` is the seq of the `attention.raised` event.
    #[serde(rename = "attention.cleared")]
    AttentionCleared {
        target: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// `stage.answered`: the stage answered a dialog on a role's
    /// screen itself — keys the user never saw, so the ledger names
    /// who pressed them. Bookkeeping, not an attention: it feeds no
    /// `!N` count, no digest and no notice, and it needs no clearing.
    #[serde(rename = "stage.answered")]
    StageAnswered {
        role: String,
        /// Which dialog was answered — [`FOLDER_TRUST`] today.
        dialog: String,
        /// The key the answer went out as (`enter` accepts).
        key: String,
        /// The working directory the dialog was answered in.
        cwd: String,
    },
    /// `ask.answered`: the stage saw `role` start a new turn in its
    /// terminal with nothing sent to it since the queued `tell user`
    /// at `target`. Clears `RoleSummary.asks` while keeping the
    /// question in `StudioSummary.exchange`.
    #[serde(rename = "ask.answered")]
    AskAnswered { role: String, target: u64 },
    /// `ask.acknowledged`: the user marked the open question at
    /// `target` seen without answering it. Clears
    /// `RoleSummary.asks` while keeping the question in the exchange.
    #[serde(rename = "ask.acknowledged")]
    AskAcknowledged { role: String, target: u64 },
    /// `role.refused`: an outward-action shim or git hook refused a
    /// command run by a role (D3) or routed it to a user card.
    #[serde(rename = "role.refused")]
    RoleRefused {
        role: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task: Option<String>,
        command: String,
        cwd: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        argv: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rule: Option<String>,
        #[serde(default, skip_serializing_if = "RefusalDisposition::is_default")]
        disposition: RefusalDisposition,
    },
    /// `outward.requested`: a role explicitly asked the person to run a
    /// carded outward write via `backlot ask --run` (push-perms spec §2.2).
    #[serde(rename = "outward.requested")]
    OutwardRequested {
        role: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task: Option<String>,
        command: String,
        cwd: String,
        argv: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        why: Option<String>,
    },
    /// `outward.completed`: the stage recorded completion, denial, or
    /// staleness of a carded outward action (`by: stage`).
    #[serde(rename = "outward.completed")]
    OutwardCompleted {
        card: u64,
        source_seq: u64,
        outcome: OutwardOutcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        output_tail: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        log_file: Option<String>,
    },
    /// `role.progress`: a role's optional one-line step update (D5).
    #[serde(rename = "role.progress")]
    RoleProgress {
        role: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task: Option<String>,
        text: String,
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
    /// `studio.reopened`: the person reopened a finished studio. Recs
    /// and cast changes work again, and the fixed seats are wanted as
    /// before. The `studio.finished` record stays in the log — it is
    /// the past, and a question it answered stays answered.
    #[serde(rename = "studio.reopened")]
    StudioReopened {
        #[serde(default)]
        reason: String,
        /// The `c<n>` marker a card's execution carries, so a restart
        /// between the append and `card.done` repeats nothing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        card: Option<String>,
    },
    /// `studio.deleted`: the user archived the finished studio's
    /// ledger directory under `archive/<id>-<stamp>`.
    #[serde(rename = "studio.deleted")]
    StudioDeleted { archive: String },
    /// `design.opened`: opens Round 1 (or `--suggest-skip`, or Round `N+1` from `waiting_user`).
    #[serde(rename = "design.opened")]
    DesignOpened {
        version: u32,
        round: u32,
        #[serde(default, skip_serializing_if = "is_false")]
        suggest_skip: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        doc_file: String,
    },
    /// `design.entry`: appends one immutable slot entry (`proposal`, `buildability`, `summary`, `user_input`, `supplement`).
    #[serde(rename = "design.entry")]
    DesignEntry {
        version: u32,
        round: u32,
        slot: DesignSlot,
        file: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<EntrySource>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        refs: Vec<u64>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        questions: Vec<DesignQuestion>,
    },
    /// `design.signed`: user-only sign-off (`approved` or `skipped`) for the current requirement version.
    #[serde(rename = "design.signed")]
    DesignSigned {
        version: u32,
        round: u32,
        mode: SignMode,
        doc_file: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// `target` is the seq of the event being corrected; `set` holds
    /// the changed fields. The earlier event stays in the log.
    #[serde(rename = "correction")]
    Correction {
        target: u64,
        reason: String,
        set: BTreeMap<String, String>,
    },
    /// Read-only placeholder for a valid line with `v <= LEDGER_VERSION`
    /// whose `kind` was added by a newer backlot.
    #[serde(skip)]
    Unknown { raw_kind: String },
}

/// `skip_serializing_if` for a bool flag that only carries meaning
/// when set — an unset flag writes nothing, and old logs with no
/// field deserialize `false`.
pub(crate) fn is_false(b: &bool) -> bool {
    !*b
}

/// Default rework cap per task (design.md "Stop conditions").
pub fn default_rework_cap() -> u32 {
    2
}

/// Default stall limit in seconds (design.md "Watching agents").
pub fn default_stall_limit_s() -> u32 {
    600
}

/// Seconds an owed step's owner may idle before one nudge (S9a).
pub fn default_nudge_after_s() -> u32 {
    180
}

/// Seconds a delivered nudge may go unanswered before the `owed`
/// attention (S9a).
pub fn default_escalate_after_s() -> u32 {
    600
}

/// The studio-wide actor cap when the cast does not set one — actors
/// playing at once, fixed roles never counting.
pub fn default_max_actors() -> u32 {
    4
}

impl EventKind {
    /// The wire name, identical to the serde tag (`"unknown"` for [`EventKind::Unknown`]).
    pub fn name(&self) -> &'static str {
        match self {
            EventKind::StudioCreated { .. } => "studio.created",
            EventKind::StudioCast { .. } => "studio.cast",
            EventKind::RoleStarted { .. } => "role.started",
            EventKind::RoleLost { .. } => "role.lost",
            EventKind::RoleResumed { .. } => "role.resumed",
            EventKind::RoleClosed { .. } => "role.closed",
            EventKind::RoleSession { .. } => "role.session",
            EventKind::RoleRecast { .. } => "role.recast",
            EventKind::RoleReopen { .. } => "role.reopen",
            EventKind::RoleRelease { .. } => "role.release",
            EventKind::RoleRestart { .. } => "role.restart",
            EventKind::ReqVersion { .. } => "req.version",
            EventKind::PlanPublished { .. } => "plan.published",
            EventKind::TaskCreated { .. } => "task.created",
            EventKind::TaskCast { .. } => "task.cast",
            EventKind::TaskPriority { .. } => "task.priority",
            EventKind::TaskDelivered { .. } => "task.delivered",
            EventKind::TaskReviewed { .. } => "task.reviewed",
            EventKind::TaskChecked { .. } => "task.checked",
            EventKind::TaskFinding { .. } => "task.finding",
            EventKind::TaskState { .. } => "task.state",
            EventKind::MessageQueued { .. } => "message.queued",
            EventKind::MessageDelivered { .. } => "message.delivered",
            EventKind::MessageDropped { .. } => "message.dropped",
            EventKind::ReportSent { .. } => "report.sent",
            EventKind::AttentionRaised { .. } => "attention.raised",
            EventKind::AttentionEscalated { .. } => "attention.escalated",
            EventKind::AttentionCleared { .. } => "attention.cleared",
            EventKind::StageAnswered { .. } => "stage.answered",
            EventKind::AskAnswered { .. } => "ask.answered",
            EventKind::AskAcknowledged { .. } => "ask.acknowledged",
            EventKind::RoleRefused { .. } => "role.refused",
            EventKind::OutwardRequested { .. } => "outward.requested",
            EventKind::OutwardCompleted { .. } => "outward.completed",
            EventKind::RoleProgress { .. } => "role.progress",
            EventKind::RoleApproval { .. } => "role.approval",
            EventKind::StudioFinished { .. } => "studio.finished",
            EventKind::StudioReopened { .. } => "studio.reopened",
            EventKind::StudioDeleted { .. } => "studio.deleted",
            EventKind::DesignOpened { .. } => "design.opened",
            EventKind::DesignEntry { .. } => "design.entry",
            EventKind::DesignSigned { .. } => "design.signed",
            EventKind::Correction { .. } => "correction",
            EventKind::Unknown { .. } => "unknown",
        }
    }

    /// The wire name, returning the original `raw_kind` for [`EventKind::Unknown`].
    pub fn raw_name(&self) -> &str {
        match self {
            EventKind::Unknown { raw_kind } => raw_kind.as_str(),
            other => other.name(),
        }
    }
}

/// The ledger schema version stamped on every `events.jsonl` line
/// (P4a). Lines from before versioning are v1 lines — serde's
/// `default_ledger_version` reads them as such — and a line stamped
/// newer is refused by the `migrate` module, never silently read.
pub const LEDGER_VERSION: u32 = 1;

/// `serde(default)` for the `v` envelope field: absent means a
/// pre-versioning line, which is a v1 line.
pub fn default_ledger_version() -> u32 {
    LEDGER_VERSION
}

/// An event as appended to the log.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
    /// Ledger schema version — always written.
    #[serde(default = "default_ledger_version")]
    pub v: u32,
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
            v: LEDGER_VERSION,
            seq,
            ts_ms,
            by: self.by,
            kind: self.kind,
        }
    }
}

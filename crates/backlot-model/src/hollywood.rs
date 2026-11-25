//! Hollywood's ledger (R21-R27, D13-D21): the overseer's own log at
//! `<state base>/hollywood/` — the same envelope as a studio's
//! (`seq`, `ts_ms`, `by`, `kind`) but its own kinds, so no studio
//! replay, list or limit ever sees it.
//!
//! - A **card** is a proposed change to a studio (D17): Hollywood
//!   proposes, the user applies or cancels, the stage carries it out
//!   (H3) and marks it done or failed. A card whose base no longer
//!   holds records `card.stale` and stays closed.
//! - `hollywood.reported` is Hollywood's own headline — the line its
//!   layer and the summary show.
//! - `message.sent` covers Hollywood's messages — from the user, to
//!   the user or to a studio's producer (R26).

use serde::{Deserialize, Serialize};

use crate::event::{ActorIdentity, ApprovalDecision, EventBy, Priority};
use crate::state::{RoleLifecycle, StudioState, TaskStatus};

// The reduced state and the append rules live in the two sibling
// files; the `hollywood::` path keeps them all.
pub use crate::hollywood_state::{
    Card, CardState, HMessage, HReport, HWRole, HollywoodState, apply, replay,
};
pub use crate::hollywood_validate::{HRefusal, validate};

/// The role key Hollywood records under — and the reserved studio
/// name, since its ledger sits beside the studios' at `<base>/hollywood`.
pub const HOLLYWOOD: &str = "hollywood";

/// Hollywood's terminal name (D13): one per core, outside the
/// studios' `bl-<studio>-<role>` shape.
pub const HOLLYWOOD_TERMINAL: &str = "bl-hollywood";

/// The last headlines kept in state (D15's collapsed layer shows one).
pub const HEADLINE_CAP: usize = 10;

// ── Events ─────────────────────────────────────────────────────

/// A stamped Hollywood ledger line — same envelope as [`crate::Event`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HollywoodEvent {
    /// Ledger schema version — always written.
    #[serde(default = "crate::event::default_ledger_version")]
    pub v: u32,
    pub seq: u64,
    pub ts_ms: u64,
    pub by: EventBy,
    #[serde(flatten)]
    pub kind: HollywoodKind,
}

/// A Hollywood event before the store stamps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewHollywoodEvent {
    pub by: EventBy,
    #[serde(flatten)]
    pub kind: HollywoodKind,
}

impl NewHollywoodEvent {
    pub fn new(by: impl Into<EventBy>, kind: HollywoodKind) -> Self {
        NewHollywoodEvent {
            by: by.into(),
            kind,
        }
    }

    pub fn stamp(self, seq: u64, ts_ms: u64) -> HollywoodEvent {
        HollywoodEvent {
            v: crate::event::LEDGER_VERSION,
            seq,
            ts_ms,
            by: self.by,
            kind: self.kind,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum HollywoodKind {
    /// A proposed change; `action`, `why` and `base` are the card.
    #[serde(rename = "card.proposed")]
    CardProposed {
        action: CardAction,
        why: String,
        base: CardBase,
    },
    /// The user confirmed the card — execution is the stage's (H3).
    #[serde(rename = "card.applied")]
    CardApplied { card: u64 },
    /// The user dropped the card; `reason` is optional.
    #[serde(rename = "card.cancelled")]
    CardCancelled {
        card: u64,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        reason: String,
    },
    /// The stage carried the action out (H3); `note` is optional.
    #[serde(rename = "card.done")]
    CardDone {
        card: u64,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        note: String,
    },
    /// The stage tried and failed; `reason` is required.
    #[serde(rename = "card.failed")]
    CardFailed { card: u64, reason: String },
    /// The base no longer held at apply time; `reason` says what moved.
    #[serde(rename = "card.stale")]
    CardStale {
        card: u64,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        reason: String,
    },
    /// Hollywood's headline, plus an optional body file under `reports/`.
    #[serde(rename = "hollywood.reported")]
    Reported {
        headline: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    /// A message to or from Hollywood. From the user it carries `text`;
    /// to a studio's producer it carries `studio` and a `file` under
    /// `messages/` (the studio ledger's `message.queued` is the actual
    /// delivery record — this is Hollywood's own copy, R26). A
    /// stage-written notice to Hollywood carries `file` under
    /// `messages/` and, when it relays a ledger event, that event's
    /// `source_seq` (and `studio` for a studio's) — the dedup key
    /// that keeps a restart from telling it twice.
    #[serde(rename = "message.sent")]
    MessageSent {
        to: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        studio: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_seq: Option<u64>,
    },
    /// The stage's lifecycle word for Hollywood's own terminal (H3):
    /// `started` when the stage ran or adopted it, `resumed` after a
    /// loss, `lost` when it vanished, `closed` when the stage stopped
    /// it on purpose — a studio's four role events folded into one
    /// kind, which is all one role needs.
    #[serde(rename = "role.state")]
    RoleState {
        state: RoleMotion,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        terminal: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        actor: Option<ActorIdentity>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// The user's answer to a permission prompt on Hollywood's own
    /// screen — the same decision a studio role's prompt records. The
    /// stage sends the runner's one-time allow key, never "don't ask
    /// again".
    #[serde(rename = "role.approval")]
    RoleApproval { decision: ApprovalDecision },
    /// The stage's guarded send handed the message to the terminal.
    #[serde(rename = "message.delivered")]
    MessageDelivered { target: u64 },
    /// A queued stage notice skipped at send time — the event it
    /// relayed no longer stands; never pending again.
    #[serde(rename = "message.dropped")]
    MessageDropped { target: u64, reason: String },
    /// A queued user tell acknowledged out-of-band via `--answers`.
    #[serde(rename = "message.seen")]
    MessageSeen { target: u64, by_seq: u64 },
    /// The user's `backlot restart --person hollywood`: stop
    /// `bl-hollywood` and launch it again — resuming its recorded
    /// session unless `fresh` asks for a new conversation. The stage
    /// carries it out on the next tick.
    #[serde(rename = "role.restart")]
    RoleRestart {
        #[serde(default, skip_serializing_if = "crate::event::is_false")]
        fresh: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Hollywood's optional one-line progress note (D5).
    #[serde(rename = "role.progress")]
    RoleProgress { text: String },
}

/// The move one `role.state` records — a studio's `role.started`,
/// `role.resumed`, `role.lost`, `role.closed` words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleMotion {
    Started,
    Resumed,
    Lost,
    Closed,
}

impl RoleMotion {
    pub fn as_str(self) -> &'static str {
        match self {
            RoleMotion::Started => "started",
            RoleMotion::Resumed => "resumed",
            RoleMotion::Lost => "lost",
            RoleMotion::Closed => "closed",
        }
    }
}

impl HollywoodKind {
    /// The ledger's `kind` string — the confirmation line's first word.
    pub fn name(&self) -> &'static str {
        match self {
            HollywoodKind::CardProposed { .. } => "card.proposed",
            HollywoodKind::CardApplied { .. } => "card.applied",
            HollywoodKind::CardCancelled { .. } => "card.cancelled",
            HollywoodKind::CardDone { .. } => "card.done",
            HollywoodKind::CardFailed { .. } => "card.failed",
            HollywoodKind::CardStale { .. } => "card.stale",
            HollywoodKind::Reported { .. } => "hollywood.reported",
            HollywoodKind::MessageSent { .. } => "message.sent",
            HollywoodKind::RoleState { .. } => "role.state",
            HollywoodKind::RoleApproval { .. } => "role.approval",
            HollywoodKind::MessageDelivered { .. } => "message.delivered",
            HollywoodKind::MessageDropped { .. } => "message.dropped",
            HollywoodKind::MessageSeen { .. } => "message.seen",
            HollywoodKind::RoleRestart { .. } => "role.restart",
            HollywoodKind::RoleProgress { .. } => "role.progress",
        }
    }
}

/// What the card does when the stage carries it out (D18): the whole
/// action set, and nothing else — no card answers a permission prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum CardAction {
    /// A complete `studio new` draft (D19): the cast and brief paths
    /// point at files in Hollywood's own `drafts/` folder.
    StudioNew {
        studio: String,
        title: String,
        /// The bound path (D4): repository, workspace directory or
        /// plain directory; member repositories are classified again
        /// at apply.
        repo: String,
        cast: String,
        brief: String,
        /// A plain directory bound with no git (D4); absent on logs
        /// that predate the field and on every other shape.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        no_git: bool,
    },
    /// Stop a finished person (`role.closed`) — frees the terminal.
    Close { studio: String, person: String },
    /// Ask a closed person back (`role.reopen`).
    Reopen { studio: String, person: String },
    /// Change a task's display priority (D11).
    Priority {
        studio: String,
        task: String,
        level: Priority,
    },
    /// Pause a running task.
    Pause { studio: String, task: String },
    /// Resume a paused or blocked task.
    Resume { studio: String, task: String },
    /// Abandon a task that is not terminal.
    Abandon { studio: String, task: String },
    /// Recast a fixed role onto a new identity (`role.recast`): close
    /// the old terminal and start a fresh conversation.
    Recast {
        studio: String,
        person: String,
        /// The full new identity; a proposal fills the permission
        /// profile from the role's current one when none is given.
        actor: ActorIdentity,
    },
    /// Change Hollywood's own actor (`[hollywood]` in config plus a
    /// fresh restart so the new agent takes over).
    Setup {
        runner: String,
        model: String,
        #[serde(default)]
        effort: Option<String>,
        #[serde(default)]
        permission_profile: Option<String>,
    },
    /// Execute a person-approved outward git push or `gh pr create`
    /// from the studio's validated work area (push-perms spec §2.2).
    Outward {
        studio: String,
        role: String,
        source_seq: u64,
        argv: Vec<String>,
        cwd: String,
        repo: String,
        remote_url: String,
        rule: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        commits: Vec<String>,
        #[serde(default)]
        commits_total: u32,
    },
}

impl CardAction {
    /// The studio the action touches.
    pub fn studio(&self) -> &str {
        match self {
            CardAction::StudioNew { studio, .. }
            | CardAction::Close { studio, .. }
            | CardAction::Reopen { studio, .. }
            | CardAction::Priority { studio, .. }
            | CardAction::Pause { studio, .. }
            | CardAction::Resume { studio, .. }
            | CardAction::Abandon { studio, .. }
            | CardAction::Recast { studio, .. }
            | CardAction::Outward { studio, .. } => studio,
            CardAction::Setup { .. } => "hollywood",
        }
    }

    /// `close c3` — the one-line action `card list` and the summary show.
    pub fn label(&self) -> String {
        match self {
            CardAction::StudioNew { studio, title, .. } => {
                format!("studio-new {studio} '{title}'")
            }
            CardAction::Close { person, .. } => format!("close {person}"),
            CardAction::Reopen { person, .. } => format!("reopen {person}"),
            CardAction::Priority { task, level, .. } => format!("priority {task} {level}"),
            CardAction::Pause { task, .. } => format!("pause {task}"),
            CardAction::Resume { task, .. } => format!("resume {task}"),
            CardAction::Abandon { task, .. } => format!("abandon {task}"),
            CardAction::Recast { person, actor, .. } => {
                format!("recast {person} → {}", crate::recast::identity_text(actor))
            }
            CardAction::Setup { .. } => {
                let actor = self.setup_actor().expect("Setup yields actor");
                format!("setup hollywood → {}", crate::recast::identity_text(&actor))
            }
            CardAction::Outward { argv, .. } => argv.join(" "),
        }
    }

    /// Target identity carried by a `Setup` card, defaulting the
    /// permission profile to `"standard"` when omitted.
    pub fn setup_actor(&self) -> Option<ActorIdentity> {
        let CardAction::Setup {
            runner,
            model,
            effort,
            permission_profile,
        } = self
        else {
            return None;
        };
        Some(ActorIdentity {
            runner: runner.clone(),
            runner_version: None,
            model: model.clone(),
            effort: effort.clone(),
            permission_profile: permission_profile
                .clone()
                .unwrap_or_else(|| "standard".into()),
        })
    }
}

/// Format an identity with spaces around `/` for card display.
pub fn format_spaced_identity(runner: &str, model: &str, effort: Option<&str>) -> String {
    match effort {
        Some(e) if !e.is_empty() => format!("{runner} / {model} / {e}"),
        _ => format!("{runner} / {model}"),
    }
}

/// Render `Hollywood: <old> → <new>` for a `Setup` card.
pub fn format_setup_transition(
    old: Option<&ActorIdentity>,
    runner: &str,
    model: &str,
    effort: Option<&str>,
) -> String {
    let new = format_spaced_identity(runner, model, effort);
    let old_str = match old {
        Some(a) => format_spaced_identity(&a.runner, &a.model, a.effort.as_deref()),
        None => "(unset)".into(),
    };
    format!("Hollywood: {old_str} → {new}")
}

/// What a card was proposed against — the fact that must still hold
/// when the user applies it. [`stale`] re-checks it against the
/// studio's state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "base", rename_all = "snake_case")]
pub enum CardBase {
    /// No such studio existed — a `studio-new` goes stale the moment
    /// the name is taken by any path.
    StudioAbsent,
    /// The person was `held` — a `close` that lands on a live,
    /// released or lost role no longer frees anything.
    PersonHeld,
    /// The person was closed and free — a `reopen` that lands on a
    /// live or already-held role is stale.
    PersonClosed,
    /// The task had this priority — a `priority` card only re-states
    /// what it saw; a different one since makes it stale.
    TaskPriority { was: Priority },
    /// The task had this status.
    TaskStatus { was: TaskStatus },
    /// The fixed role played this identity — a recast that lands on
    /// another one, or a finished studio, is stale.
    RoleIdentity { actor: ActorIdentity },
    /// Hollywood's configured identity at proposal time (`None` when
    /// `[hollywood]` was not yet set).
    HollywoodIdentity { actor: Option<ActorIdentity> },
    /// Expected git repository `HEAD` and remote ref state at proposal time.
    GitHead {
        repo: String,
        head_oid: String,
        remote_ref: String,
        remote_oid: String,
    },
}

// ── Staleness ──────────────────────────────────────────────────

/// `Some(reason)` when `base` no longer holds against `studio`
/// (`None` = the studio's ledger has no events). A card whose base
/// still stands answers `None` — the apply may proceed.
pub fn stale(action: &CardAction, base: &CardBase, studio: Option<&StudioState>) -> Option<String> {
    let name = action.studio();
    match base {
        CardBase::GitHead { .. } => {
            let CardAction::Outward { .. } = action else {
                return Some("base does not match the action".into());
            };
            let Some(s) = studio else {
                return Some(format!("no studio '{name}'"));
            };
            s.finished
                .is_some()
                .then(|| format!("studio '{name}' is finished"))
        }
        CardBase::HollywoodIdentity { .. } => match action {
            CardAction::Setup { .. } => None,
            _ => Some("base does not match the action".into()),
        },
        CardBase::StudioAbsent => studio
            .is_some()
            .then(|| format!("studio '{name}' exists now")),
        CardBase::PersonHeld => {
            let CardAction::Close { person, .. } = action else {
                return Some("base does not match the action".into());
            };
            let Some(s) = studio else {
                return Some(format!("no studio '{name}'"));
            };
            match s.roles.get(person) {
                None => Some(format!("no role '{person}'")),
                Some(r) if r.held => None,
                Some(r) => Some(format!("'{person}' is {}", lifecycle_str(r.lifecycle()))),
            }
        }
        CardBase::PersonClosed => {
            let CardAction::Reopen { person, .. } = action else {
                return Some("base does not match the action".into());
            };
            let Some(s) = studio else {
                return Some(format!("no studio '{name}'"));
            };
            match s.roles.get(person) {
                None => Some(format!("no role '{person}'")),
                Some(r) if r.held => Some(format!("'{person}' is held again")),
                Some(r) if r.lifecycle() == RoleLifecycle::Closed => None,
                Some(r) => Some(format!("'{person}' is {}", lifecycle_str(r.lifecycle()))),
            }
        }
        CardBase::TaskPriority { was } => {
            let CardAction::Priority { task, .. } = action else {
                return Some("base does not match the action".into());
            };
            let Some(s) = studio else {
                return Some(format!("no studio '{name}'"));
            };
            match s.tasks.get(task) {
                None => Some(format!("no task '{task}'")),
                Some(t) if t.priority == *was => None,
                Some(t) => Some(format!("'{task}' priority is {} now", t.priority)),
            }
        }
        CardBase::TaskStatus { was } => {
            let (CardAction::Pause { task, .. }
            | CardAction::Resume { task, .. }
            | CardAction::Abandon { task, .. }) = action
            else {
                return Some("base does not match the action".into());
            };
            let Some(s) = studio else {
                return Some(format!("no studio '{name}'"));
            };
            match s.tasks.get(task) {
                None => Some(format!("no task '{task}'")),
                Some(t) if t.status == *was => None,
                Some(t) => Some(format!("'{task}' is {} now", status_str(t.status))),
            }
        }
        CardBase::RoleIdentity { actor } => {
            let CardAction::Recast { person, .. } = action else {
                return Some("base does not match the action".into());
            };
            let Some(s) = studio else {
                return Some(format!("no studio '{name}'"));
            };
            if s.finished.is_some() {
                return Some(format!("studio '{name}' is finished"));
            }
            match s.fixed_identity(person) {
                None => Some(format!("no role '{person}'")),
                Some(current) if crate::recast::same_identity(current, actor) => None,
                Some(current) => Some(format!(
                    "'{person}' is {} now",
                    crate::recast::identity_text(current)
                )),
            }
        }
    }
}

/// Check a `Setup` card's [`CardBase::HollywoodIdentity`] against the
/// current configured `[hollywood]` identity (`None` = unset).
pub fn stale_hollywood(base: &CardBase, current: Option<&ActorIdentity>) -> Option<String> {
    let CardBase::HollywoodIdentity { actor: expected } = base else {
        return Some("base does not match setup card".into());
    };
    match (expected.as_ref(), current) {
        (None, None) => None,
        (None, Some(cur)) => Some(format!(
            "hollywood is configured as {} now",
            crate::recast::identity_text(cur)
        )),
        (Some(exp), None) => Some(format!(
            "hollywood config was cleared (expected {})",
            crate::recast::identity_text(exp)
        )),
        (Some(exp), Some(cur)) if crate::recast::same_identity(exp, cur) => None,
        (Some(_), Some(cur)) => Some(format!(
            "hollywood is {} now",
            crate::recast::identity_text(cur)
        )),
    }
}

fn lifecycle_str(l: RoleLifecycle) -> &'static str {
    match l {
        RoleLifecycle::Live => "live",
        RoleLifecycle::Lost => "lost",
        RoleLifecycle::Closed => "closed",
    }
}

/// A task status as one word — refusals and card details share it.
pub fn status_str(s: TaskStatus) -> &'static str {
    match s {
        TaskStatus::Created => "created",
        TaskStatus::Running => "running",
        TaskStatus::Delivered => "delivered",
        TaskStatus::Accepted => "accepted",
        TaskStatus::PartiallyAccepted => "partially accepted",
        TaskStatus::Paused => "paused",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Abandoned => "abandoned",
        TaskStatus::AtCap => "at cap",
        TaskStatus::TakenOver => "taken over",
    }
}

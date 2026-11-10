//! Who may record what (S9b part 1): append-time role rules keyed on
//! `by.role`, run from `validate` so every append path is covered.
//! Replay stays permissive — an old log replays whatever it holds.
//!
//! - An actor key (`actor-<task>-c<n>`) may record only its own
//!   task's delivery while it is the current cast, its own task's
//!   `blocked` state, and purpose-free `tell`/`ask` messages.
//! - The writer may record only `plan.published` and the same
//!   messages.
//! - `task.delivered` from anyone who is not that task's current
//!   actor is the user's alone — producer and director do not
//!   deliver.

use crate::event::{EventKind, NewEvent, TaskStateChange};
use crate::refusal::Refusal;
use crate::state::{StudioState, actor_task};

/// The refusal a refused role sees: `role '<r>' cannot do this; <who> can`.
fn cannot(role: &str, who: &'static str) -> Refusal {
    Refusal::RoleBound {
        role: role.into(),
        who,
    }
}

/// `task.delivered` is the task's current cast's own step —
/// `actor-<task>-c<casts.len()>` — or the user's. `check` calls it,
/// and the CLI calls it before resolving the commit range so a
/// refused role hears "cannot do this", not a git error.
pub fn may_deliver(state: &StudioState, role: &str, task: &str) -> Result<(), Refusal> {
    let allowed = match actor_task(role) {
        Some(own) if own == task => {
            let n = state.tasks.get(task).map(|t| t.casts.len()).unwrap_or(0);
            n > 0 && role == format!("actor-{task}-c{n}")
        }
        None => role == "user",
        Some(_) => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(cannot(role, "the task's current actor or the user"))
    }
}

/// The gate: whitelists for actor and writer keys, then the
/// user-only delivery rule. Runs before the per-kind state checks so
/// a refused role hears "cannot do this" ahead of any other refusal.
pub fn check(state: &StudioState, new: &NewEvent) -> Result<(), Refusal> {
    let role = new.by.role.as_str();
    if let Some(own) = actor_task(role) {
        return match &new.kind {
            EventKind::TaskDelivered { task, .. } => may_deliver(state, role, task),
            EventKind::TaskState { task, state, .. }
                if *state == TaskStateChange::Blocked && task == own =>
            {
                Ok(())
            }
            EventKind::MessageQueued { purpose: None, .. } => Ok(()),
            _ => Err(cannot(role, who_can(&new.kind))),
        };
    }
    if role == "writer" {
        let ok = matches!(
            new.kind,
            EventKind::PlanPublished { .. } | EventKind::MessageQueued { purpose: None, .. }
        );
        return if ok {
            Ok(())
        } else {
            Err(cannot(role, who_can(&new.kind)))
        };
    }
    if let EventKind::TaskDelivered { task, .. } = &new.kind {
        return may_deliver(state, role, task);
    }
    Ok(())
}

/// Who may record `kind`, for the refusal's "… can": the step's
/// owner per `docs/studio/guide.md`, matching the CLI's own gates,
/// or the stage for the ledger-internal events.
fn who_can(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::StudioCreated { .. } => "the user",
        EventKind::RoleStarted { .. }
        | EventKind::RoleLost { .. }
        | EventKind::RoleResumed { .. }
        | EventKind::RoleClosed { .. }
        | EventKind::RoleSession { .. }
        | EventKind::MessageDelivered { .. }
        | EventKind::AttentionRaised { .. }
        | EventKind::AttentionCleared { .. } => "the stage",
        EventKind::RoleReopen { .. }
        | EventKind::RoleRelease { .. }
        | EventKind::RoleApproval { .. }
        | EventKind::TaskPriority { .. } => "the user or the director",
        EventKind::ReqVersion { .. } => "the producer or the user",
        EventKind::PlanPublished { .. } => "the writer",
        EventKind::TaskDelivered { .. } => "the task's current actor or the user",
        EventKind::TaskState {
            state: TaskStateChange::Blocked,
            ..
        } => "the task's own actor or a person",
        EventKind::TaskCreated { .. }
        | EventKind::TaskCast { .. }
        | EventKind::TaskState { .. }
        | EventKind::TaskReviewed { .. }
        | EventKind::TaskFinding { .. }
        | EventKind::Correction { .. } => "the user, the producer or the director",
        EventKind::MessageQueued { purpose, .. } => {
            if purpose.is_none() {
                "a person"
            } else {
                "the producer or the stage"
            }
        }
        EventKind::ReportSent { .. } => "the director or the producer",
        EventKind::StudioFinished { .. } => "the user or the producer",
    }
}

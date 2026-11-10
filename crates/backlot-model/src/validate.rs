//! Append-time validation. `validate` runs before an event is written;
//! a `Refusal` is the one-line reason the CLI prints with exit code 1.

use crate::correct::{SeqTarget, correction_keys, correction_value_ok};
use crate::event::*;
use crate::state::{RoleLifecycle, StudioState, TaskStatus, actor_task};

pub use crate::refusal::Refusal;

fn require_studio(state: &StudioState) -> Result<&crate::state::StudioConfig, Refusal> {
    state.studio.as_ref().ok_or(Refusal::NoStudio)
}

fn require_task<'a>(state: &'a StudioState, id: &str) -> Result<&'a crate::state::Task, Refusal> {
    state
        .tasks
        .get(id)
        .ok_or_else(|| Refusal::NoTask(id.into()))
}

/// Two role keys resolve to one terminal name when both are actors on
/// the same task — a recast reuses `bl-<studio>-<task>`. Any other
/// pairing produces different names.
fn shared_terminal(a: &str, b: &str) -> bool {
    actor_task(a).is_some() && actor_task(a) == actor_task(b)
}

/// Reject events on tasks that can no longer take them.
fn check_task_open(task: &crate::state::Task, allow_paused: bool) -> Result<(), Refusal> {
    if task.status.is_terminal() {
        return Err(Refusal::Terminal {
            task: task.id.clone(),
            status: task.status,
        });
    }
    if !allow_paused && task.status == TaskStatus::Paused {
        return Err(Refusal::Paused(task.id.clone()));
    }
    Ok(())
}

/// Validate a not-yet-written event against the reduced state.
/// Corrections past the rework cap are not refused: the review is
/// recorded and the task moves to `at_cap` for the producer.
///
/// The role gate (`roles::check`) runs first — who may record what
/// is keyed on `by.role` and answers "cannot do this" ahead of any
/// state check. It never runs on replay: an old log loads whatever
/// it holds.
pub fn validate(state: &StudioState, new: &NewEvent) -> Result<(), Refusal> {
    crate::roles::check(state, new)?;
    match &new.kind {
        EventKind::StudioCreated { .. } => {
            if state.studio.is_some() {
                return Err(Refusal::StudioExists);
            }
            Ok(())
        }
        EventKind::RoleStarted { .. }
        | EventKind::RoleLost { .. }
        | EventKind::RoleResumed { .. }
        | EventKind::RoleClosed { .. }
        | EventKind::ReportSent { .. }
        | EventKind::AttentionRaised { .. }
        | EventKind::MessageQueued { .. } => {
            require_studio(state)?;
            Ok(())
        }
        // `role.session` lands on a role the log already knows —
        // the stage only looks up live roles it started.
        EventKind::RoleSession { role, .. } => {
            require_studio(state)?;
            if state.roles.contains_key(role) {
                Ok(())
            } else {
                Err(Refusal::NoRole(role.clone()))
            }
        }
        // Reopening takes a closed person with a session to resume —
        // and a free terminal name: a recast reuses `bl-<studio>-<task>`,
        // so a live sibling actor on the same task blocks it.
        EventKind::RoleReopen { role } => {
            require_studio(state)?;
            let r = state
                .roles
                .get(role)
                .ok_or_else(|| Refusal::NoRole(role.clone()))?;
            if r.lifecycle() != RoleLifecycle::Closed {
                let state = if r.live { "live" } else { "lost" };
                return Err(Refusal::RoleNotClosed {
                    role: role.clone(),
                    state,
                });
            }
            if r.session_id.is_none() {
                return Err(Refusal::RoleNoSession(role.clone()));
            }
            if r.held {
                return Err(Refusal::RoleHeld(role.clone()));
            }
            let taken = state
                .roles
                .iter()
                .find(|(k, v)| k.as_str() != role && v.live && shared_terminal(role, k));
            match taken {
                Some((other, _)) => Err(Refusal::RoleTerminalTaken {
                    role: role.clone(),
                    other: other.clone(),
                }),
                None => Ok(()),
            }
        }
        EventKind::RoleRelease { role } => {
            require_studio(state)?;
            match state.roles.get(role) {
                Some(r) if r.held => Ok(()),
                Some(_) => Err(Refusal::RoleNotHeld(role.clone())),
                None => Err(Refusal::NoRole(role.clone())),
            }
        }
        // `message.delivered` marks a queued message — or a report
        // sent to a role — as handed to its terminal.
        EventKind::MessageDelivered { target } => {
            require_studio(state)?;
            let done = match state.targets.get(target) {
                Some(SeqTarget::Message(idx)) => Some(state.messages[*idx].delivered),
                Some(SeqTarget::Report(idx)) => Some(state.reports[*idx].delivered),
                _ => None,
            };
            match done {
                Some(true) => Err(Refusal::MessageDone(*target)),
                Some(false) => Ok(()),
                None => Err(Refusal::NoMessage(*target)),
            }
        }
        EventKind::AttentionCleared { target, .. } => {
            require_studio(state)?;
            match state.targets.get(target) {
                Some(SeqTarget::Attention(idx)) => {
                    if state.attention[*idx].open {
                        Ok(())
                    } else {
                        Err(Refusal::AttentionDone(*target))
                    }
                }
                _ => Err(Refusal::NoAttention(*target)),
            }
        }
        // An approval answers an open `permission prompt` attention —
        // refusing without one keeps the stage from answering a prompt
        // nobody recorded a decision for. A flagged prompt (`BACKLOT_*`
        // or environment-clearing flags on the saved screen), or a
        // prompt on a role that is not an actor, is the user's (D9):
        // the director may not record its answer.
        EventKind::RoleApproval { role, .. } => {
            require_studio(state)?;
            let Some(attn) = state
                .open_attention()
                .find(|a| a.role == *role && a.reason == PERMISSION_PROMPT)
            else {
                return Err(Refusal::NoPermissionPrompt(role.clone()));
            };
            if new.by.role == "director" && user_owned(attn) {
                return Err(Refusal::PromptForUser {
                    role: role.clone(),
                    why: user_owned_why(attn),
                });
            }
            Ok(())
        }
        EventKind::StudioFinished { .. } => {
            require_studio(state)?;
            if state.finished.is_some() {
                return Err(Refusal::AlreadyFinished);
            }
            let mut open: Vec<String> = state
                .tasks
                .values()
                .filter(|t| !t.status.is_terminal())
                .map(|t| t.id.clone())
                .collect();
            open.sort();
            if open.is_empty() {
                Ok(())
            } else {
                Err(Refusal::UnfinishedTasks(open))
            }
        }
        EventKind::ReqVersion { version, .. } => {
            require_studio(state)?;
            let expected = state.requirement_version + 1;
            if *version != expected {
                return Err(Refusal::BadReqVersion {
                    expected,
                    got: *version,
                });
            }
            Ok(())
        }
        EventKind::PlanPublished {
            requirement_version,
            ..
        } => {
            require_studio(state)?;
            if *requirement_version == 0 || *requirement_version > state.requirement_version {
                return Err(Refusal::UnknownReqVersion(*requirement_version));
            }
            Ok(())
        }
        EventKind::TaskCreated {
            task,
            requirement_version,
            dependencies,
            ..
        } => {
            require_studio(state)?;
            if state.tasks.contains_key(task) {
                return Err(Refusal::TaskExists(task.clone()));
            }
            if *requirement_version == 0 || *requirement_version > state.requirement_version {
                return Err(Refusal::UnknownReqVersion(*requirement_version));
            }
            for dep in dependencies {
                if !state.tasks.contains_key(dep) {
                    return Err(Refusal::UnknownDependency {
                        task: task.clone(),
                        dep: dep.clone(),
                    });
                }
            }
            Ok(())
        }
        EventKind::TaskCast {
            task,
            candidate,
            actor,
            ..
        } => {
            let studio = require_studio(state)?;
            let t = require_task(state, task)?;
            check_task_open(t, false)?;
            let found = studio.candidates.iter().find(|c| c.name == *candidate);
            match found {
                None => Err(Refusal::UnknownCandidate(candidate.clone())),
                Some(c) if c.actor() != *actor => Err(Refusal::CandidateMismatch {
                    candidate: candidate.clone(),
                }),
                Some(_) => Ok(()),
            }
        }
        // Priority only orders people in the panel: any recorded
        // task takes it, open or closed.
        EventKind::TaskPriority { task, .. } => {
            require_studio(state)?;
            require_task(state, task)?;
            Ok(())
        }
        EventKind::TaskDelivered {
            task,
            delivery,
            requirement_version,
            ..
        } => {
            require_studio(state)?;
            let t = require_task(state, task)?;
            if t.status != TaskStatus::Running {
                return Err(Refusal::NotRunning {
                    task: task.clone(),
                    status: t.status,
                });
            }
            let expected = t.deliveries.len() as u32 + 1;
            if *delivery != expected {
                return Err(Refusal::BadDeliveryNumber {
                    task: task.clone(),
                    expected,
                    got: *delivery,
                });
            }
            if *requirement_version != t.requirement_version {
                return Err(Refusal::ReqPinMismatch {
                    task: task.clone(),
                    pinned: t.requirement_version,
                    got: *requirement_version,
                });
            }
            Ok(())
        }
        EventKind::TaskReviewed {
            task,
            delivery,
            verdict,
            ..
        } => {
            require_studio(state)?;
            let t = require_task(state, task)?;
            match t.status {
                TaskStatus::Delivered => {}
                // At the cap the producer resolves the task: accept
                // it (possibly with documented gaps) or partially
                // accept. More rework at the cap is refused.
                TaskStatus::AtCap => {
                    if !matches!(verdict, Verdict::Accepted | Verdict::PartiallyAccepted) {
                        return Err(Refusal::AtCapVerdict { task: task.clone() });
                    }
                }
                _ => {
                    return Err(Refusal::NotDelivered {
                        task: task.clone(),
                        status: t.status,
                    });
                }
            }
            let latest = t.deliveries.last().map(|d| d.n).unwrap_or(0);
            if *delivery != latest {
                return Err(Refusal::NotLatestDelivery {
                    task: task.clone(),
                    latest,
                    got: *delivery,
                });
            }
            Ok(())
        }
        // A finding may be recorded on any task that has that
        // delivery, including closed tasks; it never changes status.
        EventKind::TaskFinding { task, delivery, .. } => {
            require_studio(state)?;
            let t = require_task(state, task)?;
            if !t.deliveries.iter().any(|d| d.n == *delivery) {
                return Err(Refusal::NoDelivery {
                    task: task.clone(),
                    delivery: *delivery,
                });
            }
            Ok(())
        }
        EventKind::TaskState {
            task,
            state: change,
            ..
        } => {
            require_studio(state)?;
            let t = require_task(state, task)?;
            match change {
                TaskStateChange::Abandoned => {
                    if t.status == TaskStatus::Abandoned {
                        Err(Refusal::Terminal {
                            task: task.clone(),
                            status: t.status,
                        })
                    } else {
                        Ok(())
                    }
                }
                TaskStateChange::Paused => {
                    check_task_open(t, true)?;
                    if t.status == TaskStatus::Paused {
                        Err(Refusal::BadPauseResume {
                            task: task.clone(),
                            state: "paused",
                            status: t.status,
                        })
                    } else {
                        Ok(())
                    }
                }
                TaskStateChange::Resumed => {
                    // A paused task returns to what it was doing; a
                    // blocked one goes back to running with the same
                    // actor — nothing is relaunched.
                    if matches!(t.status, TaskStatus::Paused | TaskStatus::Blocked) {
                        Ok(())
                    } else {
                        Err(Refusal::BadPauseResume {
                            task: task.clone(),
                            state: "resumed",
                            status: t.status,
                        })
                    }
                }
                TaskStateChange::TakenOver | TaskStateChange::Blocked => check_task_open(t, true),
            }
        }
        EventKind::Correction { target, set, .. } => {
            require_studio(state)?;
            let Some(target_ref) = state.targets.get(target) else {
                return Err(Refusal::NoCorrectionTarget(*target));
            };
            for (key, value) in set {
                if !correction_keys(target_ref).contains(&key.as_str()) {
                    return Err(Refusal::BadCorrectionKey {
                        target: *target,
                        key: key.clone(),
                    });
                }
                if !correction_value_ok(key, value) {
                    return Err(Refusal::BadCorrectionValue {
                        target: *target,
                        key: key.clone(),
                        value: value.clone(),
                    });
                }
            }
            Ok(())
        }
    }
}

/// Whether an open `permission prompt` is the user's to answer:
/// the stage flagged the saved screen (`needs_user`), or the prompt
/// sits on a role that is not an actor — D9 delegates only an
/// actor's prompt to the director.
fn user_owned(attn: &crate::state::Attention) -> bool {
    attn.needs_user || actor_task(&attn.role).is_none()
}

/// The reason a `PromptForUser` refusal reports, naming what the
/// saved screen reached for.
fn user_owned_why(attn: &crate::state::Attention) -> String {
    if attn.needs_user {
        "its screen reached for BACKLOT_* or an environment-clearing env flag".to_string()
    } else {
        format!(
            "'{}' is not an actor; only an actor's prompt is the director's",
            attn.role
        )
    }
}

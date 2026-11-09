//! Append-time validation. `validate` runs before an event is written;
//! a `Refusal` is the one-line reason the CLI prints with exit code 1.

use std::fmt;

use crate::correct::{SeqTarget, correction_keys, correction_value_ok};
use crate::event::*;
use crate::state::{StudioState, TaskStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NoStudio,
    StudioExists,
    TaskExists(String),
    NoTask(String),
    Terminal {
        task: String,
        status: TaskStatus,
    },
    Paused(String),
    NotRunning {
        task: String,
        status: TaskStatus,
    },
    NotDelivered {
        task: String,
        status: TaskStatus,
    },
    BadDeliveryNumber {
        task: String,
        expected: u32,
        got: u32,
    },
    NotLatestDelivery {
        task: String,
        latest: u32,
        got: u32,
    },
    NoDelivery {
        task: String,
        delivery: u32,
    },
    AtCapVerdict {
        task: String,
    },
    UnknownCandidate(String),
    CandidateMismatch {
        candidate: String,
    },
    BadReqVersion {
        expected: u32,
        got: u32,
    },
    ReqPinMismatch {
        task: String,
        pinned: u32,
        got: u32,
    },
    UnknownReqVersion(u32),
    UnknownDependency {
        task: String,
        dep: String,
    },
    BadPauseResume {
        task: String,
        state: &'static str,
        status: TaskStatus,
    },
    NoMessage(u64),
    MessageDone(u64),
    NoAttention(u64),
    AttentionDone(u64),
    NoCorrectionTarget(u64),
    BadCorrectionKey {
        target: u64,
        key: String,
    },
    BadCorrectionValue {
        target: u64,
        key: String,
        value: String,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::NoStudio => {
                write!(f, "no studio recorded yet; run 'backlot studio new' first")
            }
            Refusal::StudioExists => write!(f, "studio already exists here"),
            Refusal::TaskExists(t) => write!(f, "task '{t}' already exists"),
            Refusal::NoTask(t) => write!(f, "no task '{t}'"),
            Refusal::Terminal { task, status } => {
                write!(
                    f,
                    "task '{task}' is {status:?}; finished tasks take no events"
                )
            }
            Refusal::Paused(t) => write!(f, "task '{t}' is paused"),
            Refusal::NotRunning { task, status } => {
                write!(f, "task '{task}' is {status:?}, not running")
            }
            Refusal::NotDelivered { task, status } => {
                write!(
                    f,
                    "task '{task}' is {status:?}; only a delivered task can be reviewed"
                )
            }
            Refusal::BadDeliveryNumber {
                task,
                expected,
                got,
            } => {
                write!(
                    f,
                    "task '{task}' delivery must be number {expected}, got {got}"
                )
            }
            Refusal::NotLatestDelivery { task, latest, got } => {
                write!(f, "task '{task}' latest delivery is {latest}, got {got}")
            }
            Refusal::NoDelivery { task, delivery } => {
                write!(f, "task '{task}' has no delivery {delivery}")
            }
            Refusal::AtCapVerdict { task } => {
                write!(
                    f,
                    "task '{task}' is at the rework cap; only 'accepted' or 'partially-accepted' can review it"
                )
            }
            Refusal::UnknownCandidate(c) => write!(f, "'{c}' is not a studio candidate"),
            Refusal::CandidateMismatch { candidate } => {
                write!(f, "actor identity does not match candidate '{candidate}'")
            }
            Refusal::BadReqVersion { expected, got } => {
                write!(f, "next requirement version is {expected}, got {got}")
            }
            Refusal::ReqPinMismatch { task, pinned, got } => {
                write!(
                    f,
                    "task '{task}' was dispatched under requirement v{pinned}, got v{got}"
                )
            }
            Refusal::UnknownReqVersion(v) => write!(f, "no requirement version v{v}"),
            Refusal::UnknownDependency { task, dep } => {
                write!(f, "task '{task}' depends on unknown task '{dep}'")
            }
            Refusal::BadPauseResume {
                task,
                state,
                status,
            } => {
                write!(f, "task '{task}' is {status:?}; cannot record '{state}'")
            }
            Refusal::NoMessage(seq) => write!(f, "event {seq} is not a queued message"),
            Refusal::MessageDone(seq) => write!(f, "message at {seq} is already delivered"),
            Refusal::NoAttention(seq) => write!(f, "event {seq} is not an attention raise"),
            Refusal::AttentionDone(seq) => write!(f, "attention at {seq} is already cleared"),
            Refusal::NoCorrectionTarget(seq) => {
                write!(f, "no correctable event at seq {seq}")
            }
            Refusal::BadCorrectionKey { target, key } => {
                write!(f, "event {target} has no correctable field '{key}'")
            }
            Refusal::BadCorrectionValue { target, key, value } => {
                write!(f, "event {target}: bad value '{value}' for '{key}'")
            }
        }
    }
}

impl std::error::Error for Refusal {}

fn require_studio(state: &StudioState) -> Result<&crate::state::StudioConfig, Refusal> {
    state.studio.as_ref().ok_or(Refusal::NoStudio)
}

fn require_task<'a>(state: &'a StudioState, id: &str) -> Result<&'a crate::state::Task, Refusal> {
    state
        .tasks
        .get(id)
        .ok_or_else(|| Refusal::NoTask(id.into()))
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
pub fn validate(state: &StudioState, new: &NewEvent) -> Result<(), Refusal> {
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
        | EventKind::ReportSent { .. }
        | EventKind::AttentionRaised { .. }
        | EventKind::MessageQueued { .. } => {
            require_studio(state)?;
            Ok(())
        }
        EventKind::MessageDelivered { target } => {
            require_studio(state)?;
            match state.targets.get(target) {
                Some(SeqTarget::Message(idx)) => {
                    if state.messages[*idx].delivered {
                        Err(Refusal::MessageDone(*target))
                    } else {
                        Ok(())
                    }
                }
                _ => Err(Refusal::NoMessage(*target)),
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
                    if t.status == TaskStatus::Paused {
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

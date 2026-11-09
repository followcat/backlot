//! Refusals: the one-line reasons `validate` rejects an event. The
//! CLI prints them on stderr with exit code 1.

use std::fmt;

use crate::state::TaskStatus;

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
    NoRole(String),
    RoleNotClosed {
        role: String,
        state: &'static str,
    },
    RoleNoSession(String),
    RoleHeld(String),
    RoleNotHeld(String),
    RoleTerminalTaken {
        role: String,
        other: String,
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
            Refusal::NoRole(r) => write!(f, "no role '{r}' recorded"),
            Refusal::RoleNotClosed { role, state } => {
                write!(
                    f,
                    "role '{role}' is {state}; only a closed person can be reopened"
                )
            }
            Refusal::RoleNoSession(r) => {
                write!(f, "role '{r}' has no recorded session to reopen")
            }
            Refusal::RoleHeld(r) => write!(f, "role '{r}' is already held for reopening"),
            Refusal::RoleNotHeld(r) => write!(f, "role '{r}' is not held"),
            Refusal::RoleTerminalTaken { role, other } => write!(
                f,
                "cannot reopen '{role}': live role '{other}' uses its terminal name"
            ),
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

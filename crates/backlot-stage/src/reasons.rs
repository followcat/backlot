//! Every attention reason the stage can raise, in one list. The user
//! guide's attention table is checked against [`ALL`], so a new reason
//! lands here — the docs test fails before the docs can drift.

#[cfg(test)]
pub use crate::runners::agy::AGY_PREP_FAILED;
#[cfg(test)]
pub use crate::runners::cline::CLINE_PREP_FAILED;
#[cfg(test)]
pub use crate::runners::cline::{EFFORT_NOT_SUPPORTED, MODEL_NOT_SUPPORTED};
#[cfg(test)]
pub use crate::runners::cmd::CMD_PREP_FAILED;
#[cfg(test)]
pub(crate) use crate::runners::cursor::XDG_CONFIG_HOME_SET;
pub use crate::runners::cursor::{CURSOR_DIR_OPEN, CURSOR_PREP_FAILED};
#[cfg(test)]
pub use crate::runners::devin::DEVIN_PREP_FAILED;
/// The repo already tracks a runner's hook config file, so hooks were left off.
pub const HOOKS_UNAVAILABLE: &str = "hooks unavailable";
#[cfg(test)]
pub use crate::runners::{CONFIG_MALFORMED, PROFILE_NOT_SUPPORTED, RUNNER_NOT_SUPPORTED};
#[cfg(test)]
pub(crate) use crate::watch::{SESSION_NOT_FOUND, TRUST_REFUSED};
#[cfg(test)]
pub use backlot_model::event::{
    CANNOT_RESUME, OUTWARD_ACTION, PERMISSION_PROMPT, QUESTION, STALLED,
};

/// `terminal.run` refused the role's launch; the stage retries it.
pub const LAUNCH_REFUSED: &str = "launch refused";
/// The studio's `--repo` path is not a git work tree.
pub const NOT_A_GIT_REPO: &str = "not a git repository";
/// A git step failed while preparing `work/`; the detail is git's.
pub const WORK_AREA_FAILED: &str = "work area failed";
/// A nudge went unanswered: the step is still owed and the owner
/// still idle `escalate_after_s` after the nudge was delivered (S9a).
pub const OWED: &str = "owed";
/// A running or delivered task passed `task_budget_s` (S9e) — one
/// per task ever; it clears `done` when the task is terminal.
pub const OVER_BUDGET: &str = "over budget";

/// Every attention reason the stage raises — the docs test's copy of
/// the guide's table.
#[cfg(test)]
pub const ALL: &[&str] = &[
    PERMISSION_PROMPT,
    OUTWARD_ACTION,
    QUESTION,
    STALLED,
    CANNOT_RESUME,
    SESSION_NOT_FOUND,
    TRUST_REFUSED,
    LAUNCH_REFUSED,
    NOT_A_GIT_REPO,
    WORK_AREA_FAILED,
    RUNNER_NOT_SUPPORTED,
    PROFILE_NOT_SUPPORTED,
    CONFIG_MALFORMED,
    CURSOR_PREP_FAILED,
    CURSOR_DIR_OPEN,
    HOOKS_UNAVAILABLE,
    XDG_CONFIG_HOME_SET,
    CMD_PREP_FAILED,
    CLINE_PREP_FAILED,
    AGY_PREP_FAILED,
    DEVIN_PREP_FAILED,
    MODEL_NOT_SUPPORTED,
    EFFORT_NOT_SUPPORTED,
    OWED,
    OVER_BUDGET,
    crate::ratelimit::RATE_LIMITED,
    crate::ratelimit::AGENT_ERROR,
    crate::connerr::CONNECTION_ERROR,
];

#[cfg(test)]
#[path = "reasons_tests.rs"]
mod tests;

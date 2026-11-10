//! Every attention reason the stage can raise, in one list. The user
//! guide's attention table is checked against [`ALL`], so a new reason
//! lands here — the docs test fails before the docs can drift.

#[cfg(test)]
pub use crate::runners::{CONFIG_MALFORMED, PROFILE_NOT_SUPPORTED, RUNNER_NOT_SUPPORTED};
#[cfg(test)]
pub(crate) use crate::watch::SESSION_NOT_FOUND;
#[cfg(test)]
pub use backlot_model::event::{CANNOT_RESUME, PERMISSION_PROMPT, STALLED};

/// `terminal.run` refused the role's launch; the stage retries it.
pub const LAUNCH_REFUSED: &str = "launch refused";
/// The studio's `--repo` path is not a git work tree.
pub const NOT_A_GIT_REPO: &str = "not a git repository";
/// A git step failed while preparing `work/`; the detail is git's.
pub const WORK_AREA_FAILED: &str = "work area failed";
/// A nudge went unanswered: the step is still owed and the owner
/// still idle `escalate_after_s` after the nudge was delivered (S9a).
pub const OWED: &str = "owed";

/// Every attention reason the stage raises — the docs test's copy of
/// the guide's table.
#[cfg(test)]
pub const ALL: &[&str] = &[
    PERMISSION_PROMPT,
    STALLED,
    CANNOT_RESUME,
    SESSION_NOT_FOUND,
    LAUNCH_REFUSED,
    NOT_A_GIT_REPO,
    WORK_AREA_FAILED,
    RUNNER_NOT_SUPPORTED,
    PROFILE_NOT_SUPPORTED,
    CONFIG_MALFORMED,
    OWED,
];

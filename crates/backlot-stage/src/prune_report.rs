//! The typed report of `backlot studio prune` — what the ack carries
//! and the `--json` printer serializes.

use serde::{Deserialize, Serialize};

/// One thing prune did — or, in a dry run, would do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum Step {
    RemovedWorktree(String),
    PrunedWorktree(String),
    KeptWorktree(String, String),
    DeletedBranch(String),
    KeptBranch(String, String),
    /// A recorded member repository git could not look at — kept
    /// while the other members still run.
    KeptRepo(String, String),
}

/// One studio's block in a prune report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PruneStudioReport {
    pub studio: String,
    pub finished: bool,
    pub dry_run: bool,
    pub steps: Vec<Step>,
}

/// What `backlot studio prune` did or planned — the typed result the
/// ack carries and the `--json` printer serializes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PruneReport {
    pub studios: Vec<PruneStudioReport>,
}

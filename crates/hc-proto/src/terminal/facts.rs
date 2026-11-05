// Derived from herdr-core src/repository.rs, src/activity.rs and
// schema/hc.openrpc.json (Apache-2.0).

//! Origin facts. Keys and paths are opaque machine-side values; null means
//! unknown, never zero. Ordinals are comparable only within one origin,
//! process epoch and scope, not across machines.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// A full fact replacement must contain nullable members. A bare Option
// silently accepts omission, which could erase a previously known fact.
fn nullable<'de, D, T>(de: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(de)
}

/// A complete repository observation, replaced as a unit by events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryFacts {
    /// Origin core, independent of relay aliases.
    pub core_id: String,
    /// Observed foreground cwd, falling back to launch cwd at the origin.
    pub cwd: String,
    /// `pending`, `ready`, `stale`, `unavailable`, or a future state.
    pub state: String,
    /// Origin-provided automatic label.
    #[serde(deserialize_with = "nullable")]
    pub auto_label: Option<String>,
    /// Repository and checkout identity, absent for a plain directory.
    #[serde(deserialize_with = "nullable")]
    pub checkout: Option<Checkout>,
    /// Branch, unknown for detached or unreadable HEAD.
    #[serde(deserialize_with = "nullable")]
    pub branch: Option<String>,
    /// Unknown for missing upstream, partial clones, or failed probes.
    #[serde(deserialize_with = "nullable")]
    pub ahead_behind: Option<AheadBehind>,
}

/// Repository identity and its particular checkout on the origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkout {
    /// Group only together with `RepositoryFacts.core_id`.
    pub repository_key: String,
    /// Distinguishes linked worktrees within a repository.
    pub checkout_key: String,
    /// Origin's repository display name.
    pub repo_name: String,
    /// Origin's checkout display name.
    pub checkout_name: String,
    /// Origin's checkout root, never resolved by the client.
    pub repo_root: String,
    /// This checkout is a linked worktree.
    pub is_linked_worktree: bool,
    /// This checkout is bare.
    pub is_bare: bool,
}

/// Known upstream divergence, including observed zero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AheadBehind {
    /// Commits ahead of upstream.
    pub ahead: u64,
    /// Commits behind upstream.
    pub behind: u64,
}

/// A complete record clock set, replaced atomically with status updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    /// Origin core id.
    pub core_id: String,
    /// Fresh process epoch, including after restore.
    pub epoch: String,
    /// Record lifetime; reset invalidates client-local acknowledgement.
    pub scope: u64,
    /// Latest registry revision, not a lifetime or global ordering key.
    pub revision: u64,
    /// Root is the empty string, retained even with a null status.
    pub records: BTreeMap<String, RecordClock>,
}

/// One record's origin-local clocks. Initial readiness is not completion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordClock {
    /// Last state transition, or unknown.
    #[serde(deserialize_with = "nullable")]
    pub state_ordinal: Option<u64>,
    /// Last genuine completion, or unknown/reset.
    #[serde(deserialize_with = "nullable")]
    pub completion_ordinal: Option<u64>,
}

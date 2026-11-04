// Derived from herdr-core schema/hc.openrpc.json (Apache-2.0).

//! Machines this core has added, as `machine.list` reports them. States stay
//! plain strings so a newer core's states parse instead of failing.

use serde::{Deserialize, Serialize};

/// An added machine's profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    /// What this core calls it: `host-b` in `host-b/t7`.
    pub name: String,
    /// How it is reached: an ssh target, or (`via`) a command.
    pub target: String,
    /// The other core's permanent id.
    pub id: String,
    /// What that core calls itself (its hostname).
    pub host: String,
    /// Its `hc` version when last seen.
    pub version: String,
    /// The hc session there (null: its default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// Short-lived (a sandbox for a task).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub temporary: bool,
    /// `target` is a command that runs a shell line there, like ssh.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub via: bool,
}

/// A machine as `machine.list` shows it: its profile and its line now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineView {
    /// What this core calls it: `host-b` in `host-b/t7`.
    pub name: String,
    /// How it is reached: an ssh target, or (`via`) a command.
    pub target: String,
    /// The other core's permanent id.
    pub id: String,
    /// What that core calls itself (its hostname).
    pub host: String,
    /// Its `hc` version when last seen.
    pub version: String,
    /// `connecting`, `connected`, `needs_login` or `unreachable`; kept as a
    /// string for newer states.
    pub state: String,
    /// The hc session there (null: its default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// Since when it is not connected, ms since the epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_ms: Option<u64>,
    /// Short-lived (a sandbox for a task).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub temporary: bool,
    /// `target` is a command that runs a shell line there, like ssh.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub via: bool,
    /// Why the line is down, when the core said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// The `machine.list` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineListResult {
    /// Every machine this core has added, connected or not.
    pub machines: Vec<MachineView>,
}

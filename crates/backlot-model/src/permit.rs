//! Remembered permission grants (permits step 1): narrow shell or edit
//! rules a person explicitly allowed for one role key or for the studio.

use serde::{Deserialize, Serialize};

/// How widely a grant applies. `Studio` reaches every fixed role and
/// every actor candidate in the studio; `Role` reaches only `role`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermitScope {
    Role,
    Studio,
}

/// What a grant covers: a shell command stem or a file-edit path rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermitKind {
    Shell,
    Edit,
}

/// One active grant reduced from `permit.granted`; a `permit.revoked`
/// event removes the entry, so this map is the native writers' input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermitRecord {
    pub id: String,
    pub scope: PermitScope,
    /// The role key for `scope == role`: a fixed role, `hollywood`, or
    /// an actor candidate name. Absent on a studio grant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub kind: PermitKind,
    pub rule: String,
    /// Outward actions may be remembered for a role, never a studio.
    #[serde(default, skip_serializing_if = "crate::event::is_false")]
    pub outward: bool,
    /// The permission-prompt attention seq the grant came from.
    pub from: u64,
    pub runner: String,
    pub seq: u64,
    pub ts_ms: u64,
}

impl PermitRecord {
    /// Whether this grant reaches `role_key`.
    pub fn reaches(&self, role_key: &str) -> bool {
        self.scope == PermitScope::Studio || self.role.as_deref() == Some(role_key)
    }
}

/// The role key a grant is attached to: fixed roles keep their names,
/// Hollywood is itself, and an actor uses its candidate name so later
/// tasks cast to the same candidate inherit the grant.
pub fn role_key(state: &crate::state::StudioState, role: &str) -> String {
    if let Some(task) = crate::state::actor_task(role)
        && let Some(t) = state.tasks.get(task)
        && let Some(cast) = &t.cast
        && !cast.candidate.is_empty()
    {
        return cast.candidate.clone();
    }
    role.to_string()
}

/// Active grants reaching `role_key`, oldest first.
pub fn grants_for<'a>(
    state: &'a crate::state::StudioState,
    role_key: &str,
) -> Vec<&'a PermitRecord> {
    state
        .permits
        .values()
        .filter(|p| p.reaches(role_key))
        .collect()
}

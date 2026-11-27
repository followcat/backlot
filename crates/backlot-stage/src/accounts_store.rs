//! Persistent per-account quota stall state under `<state_dir>/accounts.json`.
//!
//! Written atomically via temp-file + rename only on transitions so ticks do not
//! churn disk I/O, and surviving stage daemon restarts cleanly. Missing or corrupt
//! files degrade gracefully without failing stage startup or CLI commands.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use backlot_model::summary::QuotaWindow;
use serde::{Deserialize, Serialize};

pub const ACCOUNTS_SCHEMA: &str = "backlot-accounts/1";
pub const ACCOUNTS_FILE: &str = "accounts.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountsFile {
    #[serde(default = "default_schema")]
    pub schema: String,
    #[serde(default)]
    pub accounts: BTreeMap<String, PersistedAccount>,
}

impl Default for AccountsFile {
    fn default() -> Self {
        Self {
            schema: ACCOUNTS_SCHEMA.into(),
            accounts: BTreeMap::new(),
        }
    }
}

fn default_schema() -> String {
    ACCOUNTS_SCHEMA.into()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedAccount {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub runner: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<PersistedRole>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub stalled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stalled_since_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_stall_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<QuotaWindow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stalled_by: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PersistedRole {
    pub studio: String,
    pub role: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
}

fn is_false(b: &bool) -> bool {
    !*b
}

pub(crate) fn path(state_dir: &Path) -> PathBuf {
    state_dir.join(ACCOUNTS_FILE)
}

pub(crate) fn role_ref(studio: &str, role: &str) -> String {
    if studio == "hollywood" && role == "hollywood" {
        "hollywood".into()
    } else {
        format!("{studio}/{role}")
    }
}

/// Read `<state_dir>/accounts.json`. Missing files return `(default, None)`; corrupt
/// or unreadable files return `(default, Some(warning))` so callers tolerate them.
pub(crate) fn load(state_dir: &Path) -> (AccountsFile, Option<String>) {
    let dest = path(state_dir);
    let bytes = match fs::read(&dest) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (AccountsFile::default(), None);
        }
        Err(e) => {
            return (
                AccountsFile::default(),
                Some(format!("cannot read {}: {e}", dest.display())),
            );
        }
    };
    match serde_json::from_slice::<AccountsFile>(&bytes) {
        Ok(mut parsed) => {
            if parsed.schema.is_empty() {
                parsed.schema = ACCOUNTS_SCHEMA.into();
            }
            (parsed, None)
        }
        Err(e) => (
            AccountsFile::default(),
            Some(format!("ignoring corrupt {}: {e}", dest.display())),
        ),
    }
}

/// Atomically persist `<state_dir>/accounts.json` via temp-file + rename (0600 on Unix).
pub(crate) fn save(state_dir: &Path, data: &AccountsFile) -> std::io::Result<()> {
    fs::create_dir_all(state_dir)?;
    let bytes = serde_json::to_vec_pretty(data).map_err(std::io::Error::other)?;
    let dest = path(state_dir);
    let tmp = state_dir.join(format!(".{ACCOUNTS_FILE}.tmp-{}", std::process::id()));
    fs::write(&tmp, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(&tmp, dest)
}

/// Remove `role_key` from `stalled_by` inside `<state_dir>/accounts.json` when present
/// (used by synchronous CLI commands such as `attention clear`).
pub(crate) fn clear_role_in_file(state_dir: &Path, role_key: &str) {
    if !path(state_dir).exists() {
        return;
    }
    let (mut data, warn) = load(state_dir);
    if warn.is_some() {
        return;
    }
    if data.clear_role_stall(role_key) {
        let _ = save(state_dir, &data);
    }
}

impl AccountsFile {
    /// Record that `(studio, role, model)` uses `account_id`. Returns `true` if state changed.
    pub(crate) fn observe_role(
        &mut self,
        account_id: &str,
        runner: &str,
        source: &str,
        studio: &str,
        role: &str,
        model: &str,
    ) -> bool {
        let prev = self.accounts.get(account_id).cloned();
        let entry = self.accounts.entry(account_id.to_string()).or_default();
        if entry.runner.is_empty() || entry.runner == runner {
            entry.runner = runner.to_string();
        }
        if !source.is_empty() && entry.source != source {
            entry.source = source.to_string();
        }
        if let Some(existing) = entry
            .roles
            .iter_mut()
            .find(|r| r.studio == studio && r.role == role)
        {
            if !model.is_empty() && existing.model != model {
                existing.model = model.to_string();
            }
        } else {
            entry.roles.push(PersistedRole {
                studio: studio.to_string(),
                role: role.to_string(),
                model: model.to_string(),
            });
            entry.roles.sort();
        }
        prev.as_ref() != Some(entry)
    }

    /// Record an active quota stall on `account_id` triggered by `role_key`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_stall(
        &mut self,
        account_id: &str,
        runner: &str,
        source: &str,
        role_key: &str,
        now_ms: u64,
        reset_at_ms: Option<u64>,
        window: QuotaWindow,
        detail: &str,
    ) -> bool {
        let prev = self.accounts.get(account_id).cloned();
        let entry = self.accounts.entry(account_id.to_string()).or_default();
        if entry.runner.is_empty() {
            entry.runner = runner.to_string();
        }
        if !source.is_empty() {
            entry.source = source.to_string();
        }
        entry.stalled = true;
        if entry.stalled_since_ms.is_none() {
            entry.stalled_since_ms = Some(now_ms);
        }
        entry.last_stall_ms = Some(
            entry
                .last_stall_ms
                .map_or(now_ms, |existing| existing.max(now_ms)),
        );
        if let Some(reset) = reset_at_ms {
            entry.reset_at_ms = Some(reset);
        }
        if window != QuotaWindow::Unknown || entry.window.is_none() {
            entry.window = Some(window);
        }
        if !entry.stalled_by.iter().any(|r| r == role_key) {
            entry.stalled_by.push(role_key.to_string());
            entry.stalled_by.sort();
        }
        if !detail.is_empty() {
            entry.detail = detail.to_string();
        }
        prev.as_ref() != Some(entry)
    }

    pub(crate) fn has_stalled_role(&self, role_key: &str) -> bool {
        self.accounts
            .values()
            .any(|a| a.stalled_by.iter().any(|r| r == role_key))
    }

    /// Clear `role_key` from any `stalled_by` list; clears `stalled` when empty while
    /// preserving `last_stall_ms` and prior `reset_at_ms`.
    pub(crate) fn clear_role_stall(&mut self, role_key: &str) -> bool {
        let mut changed = false;
        for entry in self.accounts.values_mut() {
            let before_len = entry.stalled_by.len();
            entry.stalled_by.retain(|r| r != role_key);
            if entry.stalled_by.len() != before_len {
                changed = true;
            }
            if entry.stalled && entry.stalled_by.is_empty() {
                entry.stalled = false;
                entry.stalled_since_ms = None;
                changed = true;
            }
        }
        changed
    }

    /// Apply expiry (`now_ms >= reset_at_ms`) and reconcile `stalled_by` against
    /// open `rate limited` studio attentions plus sampled Hollywood quota status.
    pub(crate) fn reconcile(
        &mut self,
        now_ms: u64,
        active_stalled_studio_roles: &BTreeSet<String>,
        hollywood_rate_limited: Option<bool>,
    ) -> bool {
        let mut changed = false;
        for entry in self.accounts.values_mut() {
            if !entry.stalled {
                continue;
            }
            if entry.reset_at_ms.is_some_and(|r| now_ms >= r) {
                entry.stalled = false;
                entry.stalled_since_ms = None;
                entry.stalled_by.clear();
                changed = true;
                continue;
            }
            let before_len = entry.stalled_by.len();
            entry.stalled_by.retain(|r| {
                if r == "hollywood" {
                    hollywood_rate_limited.unwrap_or(true)
                } else {
                    active_stalled_studio_roles.contains(r)
                }
            });
            if entry.stalled_by.len() != before_len {
                changed = true;
            }
            if entry.stalled_by.is_empty() {
                entry.stalled = false;
                entry.stalled_since_ms = None;
                changed = true;
            }
        }
        changed
    }
}

#[cfg(test)]
#[path = "accounts_store_tests.rs"]
mod tests;

//! Make-before-break handoff persistent state (`<hdir>/handoff.json`) and
//! quota/rate/auth/connection block state types (§2.3).

use std::fs;
use std::path::{Path, PathBuf};

use backlot_model::event::ActorIdentity;
use backlot_model::summary::{HandoffSummary, QueueReason};
use serde::{Deserialize, Serialize};

use crate::store;

pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 90_000;
pub(crate) const PREVIOUS_RETENTION_MS: u64 = 600_000;
pub(crate) const COMMITTED_SUMMARY_RETENTION_MS: u64 = 15_000;

/// Retention window for `hollywood-previous` tombstones — defaults to 10 minutes
/// ([`PREVIOUS_RETENTION_MS`]), overridable via `BACKLOT_HOLLYWOOD_PREVIOUS_MS`
/// (clamped to `>= 50 ms`) for integration tests.
pub(crate) fn previous_retention_ms(env: &std::collections::BTreeMap<String, String>) -> u64 {
    env.get("BACKLOT_HOLLYWOOD_PREVIOUS_MS")
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|ms| ms.max(50))
        .unwrap_or(PREVIOUS_RETENTION_MS)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HandoffPhase {
    Preparing,
    Starting,
    Verifying,
    NeedsApproval,
    Committing,
    Committed,
    RolledBack,
}

impl HandoffPhase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            HandoffPhase::Preparing => "preparing",
            HandoffPhase::Starting => "starting",
            HandoffPhase::Verifying => "verifying",
            HandoffPhase::NeedsApproval => "needs_approval",
            HandoffPhase::Committing => "committing",
            HandoffPhase::Committed => "committed",
            HandoffPhase::RolledBack => "rolled_back",
        }
    }

    pub(crate) fn is_active(self) -> bool {
        matches!(
            self,
            HandoffPhase::Preparing
                | HandoffPhase::Starting
                | HandoffPhase::Verifying
                | HandoffPhase::NeedsApproval
                | HandoffPhase::Committing
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HandoffRecord {
    pub id: String,
    pub phase: HandoffPhase,
    pub generation: u64,
    pub target_runner: String,
    pub target_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_effort: Option<String>,
    #[serde(default)]
    pub force: bool,
    pub trigger: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_terminal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_signals: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_cursor_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_actor: Option<ActorIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_terminal: Option<String>,
    #[serde(default)]
    pub requested_ms: u64,
    #[serde(default)]
    pub started_ms: u64,
    #[serde(default)]
    pub started_seq: u64,
    pub timeout_ms: u64,
    #[serde(default)]
    pub active_elapsed_ms: u64,
    #[serde(default)]
    pub last_tick_ms: u64,
    #[serde(default)]
    pub saw_working_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saw_working_start_ms: Option<u64>,
    #[serde(default)]
    pub saw_hook_activity: bool,
    #[serde(default)]
    pub cancel_requested: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_base_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_ms: Option<u64>,
}

impl Default for HandoffRecord {
    fn default() -> Self {
        HandoffRecord {
            id: String::new(),
            phase: HandoffPhase::RolledBack,
            generation: 0,
            target_runner: String::new(),
            target_model: String::new(),
            target_effort: None,
            force: false,
            trigger: String::new(),
            card_seq: None,
            candidate_name: None,
            candidate_terminal: None,
            candidate_session: None,
            candidate_signals: None,
            candidate_cursor_dir: None,
            from_actor: None,
            old_terminal: None,
            requested_ms: 0,
            started_ms: 0,
            started_seq: 0,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            active_elapsed_ms: 0,
            last_tick_ms: 0,
            saw_working_ms: 0,
            saw_working_start_ms: None,
            saw_hook_activity: false,
            cancel_requested: false,
            approval_detail: None,
            approval_base_seq: None,
            brief_path: None,
            error: None,
            finished_ms: None,
        }
    }
}

impl HandoffRecord {
    pub(crate) fn to_summary(&self) -> HandoffSummary {
        HandoffSummary {
            id: self.id.clone(),
            phase: self.phase.as_str().to_string(),
            target_runner: self.target_runner.clone(),
            target_model: self.target_model.clone(),
            target_effort: self.target_effort.clone(),
            candidate_terminal: self.candidate_terminal.clone(),
            started_ms: self.started_ms.max(self.requested_ms),
            timeout_ms: self.timeout_ms,
            error: self.error.clone(),
        }
    }

    pub(crate) fn visible_in_summary(&self, now_ms: u64) -> bool {
        if self.phase.is_active() {
            return true;
        }
        match self.phase {
            HandoffPhase::Committed => self
                .finished_ms
                .is_none_or(|t| now_ms.saturating_sub(t) <= COMMITTED_SUMMARY_RETENTION_MS),
            HandoffPhase::RolledBack => self
                .finished_ms
                .is_none_or(|t| now_ms.saturating_sub(t) <= PREVIOUS_RETENTION_MS),
            _ => true,
        }
    }
}

/// Active quota/rate/auth/connection block state for Hollywood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HBlockedKind {
    RateLimited { detail: String },
    OutOfQuota { detail: String },
    AuthError { detail: String },
    ConnectionError { detail: String },
    AgentError { detail: String },
}

impl HBlockedKind {
    pub(crate) fn blocks_delivery(&self) -> bool {
        true
    }

    pub(crate) fn queue_reason(&self) -> (QueueReason, Option<String>) {
        match self {
            HBlockedKind::RateLimited { detail } => {
                (QueueReason::RateLimited, Some(detail.clone()))
            }
            HBlockedKind::OutOfQuota { detail } => (QueueReason::OutOfQuota, Some(detail.clone())),
            HBlockedKind::AuthError { detail } => (QueueReason::AuthError, Some(detail.clone())),
            HBlockedKind::ConnectionError { detail } => {
                (QueueReason::RateLimited, Some(detail.clone()))
            }
            HBlockedKind::AgentError { detail } => (QueueReason::AuthError, Some(detail.clone())),
        }
    }

    pub(crate) fn attention_fields(&self) -> (&'static str, String) {
        match self {
            HBlockedKind::OutOfQuota { detail } => ("out of quota", detail.clone()),
            HBlockedKind::AuthError { detail } => ("auth error", detail.clone()),
            HBlockedKind::RateLimited { detail } => ("rate limited", detail.clone()),
            HBlockedKind::ConnectionError { detail } => ("connection error", detail.clone()),
            HBlockedKind::AgentError { detail } => ("agent error", detail.clone()),
        }
    }
}

pub(crate) fn handoff_path(hdir: &Path) -> PathBuf {
    hdir.join("handoff.json")
}

pub(crate) fn load_handoff(hdir: &Path) -> Result<Option<HandoffRecord>, String> {
    let p = handoff_path(hdir);
    let raw = match fs::read_to_string(&p) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", p.display())),
    };
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|e| format!("{}: {e}", p.display()))
}

pub(crate) fn save_handoff(hdir: &Path, rec: &HandoffRecord) -> Result<(), String> {
    fs::create_dir_all(hdir).map_err(|e| format!("{}: {e}", hdir.display()))?;
    let p = handoff_path(hdir);
    let tmp = hdir.join(".handoff.json.tmp");
    let body = serde_json::to_vec_pretty(rec).map_err(|e| e.to_string())?;
    write_private_bytes(&tmp, &body).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, &p).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("{}: {e}", p.display())
    })
}

#[cfg(unix)]
fn write_private_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

#[cfg(not(unix))]
fn write_private_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    fs::write(path, bytes)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn stage_handoff_request(
    hdir: &Path,
    runner: &str,
    model: &str,
    effort: Option<&str>,
    timeout_ms: u64,
    force: bool,
    trigger: &str,
    card_seq: Option<u64>,
    now_ms: u64,
) -> Result<HandoffRecord, String> {
    fs::create_dir_all(hdir).map_err(|e| format!("{}: {e}", hdir.display()))?;
    let _lock = store::take_lock(hdir).map_err(|e| format!("lock {}: {e}", hdir.display()))?;
    let existing = load_handoff(hdir)?;
    if let Some(cur) = &existing
        && cur.phase.is_active()
    {
        return Err("handoff already in progress".into());
    }
    let generation = existing.map_or(1, |e| e.generation.saturating_add(1));
    let id = format!("h-{now_ms}-{generation}");
    let rec = HandoffRecord {
        id,
        phase: HandoffPhase::Preparing,
        generation,
        target_runner: runner.to_string(),
        target_model: model.to_string(),
        target_effort: effort.map(str::to_string),
        force,
        trigger: trigger.to_string(),
        card_seq,
        requested_ms: now_ms,
        timeout_ms: timeout_ms.max(1_000),
        ..Default::default()
    };
    save_handoff(hdir, &rec)?;
    Ok(rec)
}

pub(crate) fn request_cancel(hdir: &Path) -> Result<HandoffRecord, String> {
    fs::create_dir_all(hdir).map_err(|e| format!("{}: {e}", hdir.display()))?;
    let _lock = store::take_lock(hdir).map_err(|e| format!("lock {}: {e}", hdir.display()))?;
    let mut rec = load_handoff(hdir)?.ok_or_else(|| "no active handoff to cancel".to_string())?;
    if !rec.phase.is_active() {
        return Err(format!(
            "handoff '{}' is already {}",
            rec.id,
            rec.phase.as_str()
        ));
    }
    rec.cancel_requested = true;
    save_handoff(hdir, &rec)?;
    Ok(rec)
}

pub(crate) fn handoff_blocks_delivery(hdir: &Path) -> bool {
    load_handoff(hdir)
        .ok()
        .flatten()
        .is_some_and(|r| r.phase.is_active())
}

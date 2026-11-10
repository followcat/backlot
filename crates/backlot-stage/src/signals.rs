//! Hook signals (S9f): the per-session settings file points every
//! Claude Code hook at `backlot hook --file <role's signals.jsonl>`,
//! which appends one JSON line per event. The stage reads the
//! file's tail — the last 16 KB — and the last line from the
//! role's own session is the role's first work source, ahead of
//! the screen sample: `PermissionRequest`, or a `permission_prompt`
//! notification, is `Blocked`; `UserPromptSubmit`, `PreToolUse`,
//! `PostToolUse` and `PostToolUseFailure` are `Working`; `Stop`,
//! `StopFailure` and an `idle_prompt` notification are `Waiting`.
//! A session that sent nothing yet has no signal state — the
//! screen answers alone.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use backlot_model::state::StudioState;
use backlot_model::summary::Work;
use serde::{Deserialize, Serialize};

/// The signals file's read window: the stage looks at the last
/// 16 KB only, so a busy session's file stays cheap.
pub const TAIL_BYTES: u64 = 16 * 1024;

/// `<studio>/roles/<role>/signals.jsonl` — the hook command's
/// `--file`.
pub fn signals_path(dir: &Path, role: &str) -> PathBuf {
    dir.join("roles").join(role).join("signals.jsonl")
}

/// The last line a session sent — the fields the stage uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    #[serde(default)]
    pub ts_ms: u64,
    #[serde(default)]
    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<serde_json::Value>,
}

impl Signal {
    /// The work state the event reports — `None` for an event with
    /// no mapping; the screen answers then.
    pub fn work(&self) -> Option<Work> {
        match self.event.as_str() {
            "PermissionRequest" => Some(Work::Blocked),
            "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => {
                Some(Work::Working)
            }
            "Stop" | "StopFailure" => Some(Work::Waiting),
            "Notification" => match self.notification_type.as_deref() {
                Some("permission_prompt") => Some(Work::Blocked),
                Some("idle_prompt") => Some(Work::Waiting),
                _ => None,
            },
            _ => None,
        }
    }

    /// The command or path a `PermissionRequest` names — Bash's
    /// `command`, else a file tool's `file_path`.
    pub fn tool_target(&self) -> Option<&str> {
        let input = self.tool_input.as_ref()?;
        input
            .get("command")
            .and_then(|v| v.as_str())
            .or_else(|| input.get("file_path").and_then(|v| v.as_str()))
    }
}

/// The file's last [`TAIL_BYTES`] as text, the partial first line
/// dropped. A missing or unreadable file reads as empty.
pub fn tail(path: &Path) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let start = f
        .metadata()
        .map(|m| m.len().saturating_sub(TAIL_BYTES))
        .unwrap_or(0);
    let mut buf = Vec::new();
    if f.seek(SeekFrom::Start(start)).is_err() || f.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&buf);
    if start == 0 {
        return text.into_owned();
    }
    match text.find('\n') {
        Some(i) => text[i + 1..].to_string(),
        None => String::new(),
    }
}

/// One line as a [`Signal`], when it parses and names `session_id`
/// — `None` for another session's line or an unparseable one.
fn parse(line: &str, session_id: &str) -> Option<Signal> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    (v.get("session_id").and_then(|s| s.as_str()) == Some(session_id)).then(|| Signal {
        ts_ms: v.get("ts_ms").and_then(|t| t.as_u64()).unwrap_or(0),
        event: v
            .get("event")
            .and_then(|e| e.as_str())
            .unwrap_or_default()
            .into(),
        notification_type: v
            .get("notification_type")
            .and_then(|n| n.as_str())
            .map(String::from),
        error: v.get("error").and_then(|e| e.as_str()).map(String::from),
        tool_name: v
            .get("tool_name")
            .and_then(|n| n.as_str())
            .map(String::from),
        tool_input: v.get("tool_input").cloned(),
    })
}

/// The last line `session_id` sent in `tail` that maps to a work
/// state — `None` when the session sent none. An event that maps
/// to nothing, such as an `auth_success` notification, is skipped:
/// it is not a reason to hand the role back to the screen.
pub fn last_work(tail: &str, session_id: &str) -> Option<Signal> {
    tail.lines()
        .rev()
        .filter_map(|line| parse(line, session_id))
        .find(|s| s.work().is_some())
}

/// The request the role is still waiting on: the first
/// non-`Notification` line back from the end, when it is a
/// `PermissionRequest`. A `permission_prompt` notification is the
/// same prompt's six-second chime, not a change of prompt — any
/// other event ends the request.
pub fn pending_request(tail: &str, session_id: &str) -> Option<Signal> {
    for line in tail.lines().rev() {
        let Some(s) = parse(line, session_id) else {
            continue;
        };
        if s.event == "Notification" {
            continue;
        }
        return (s.event == "PermissionRequest").then_some(s);
    }
    None
}

/// Signals first, the screen as fallback (S9f): `Blocked` wins from
/// either source — a prompt can come before its signal does —
/// otherwise the signal's state when there is one, the screen's
/// when there is none.
pub fn combine(signal: Option<Work>, screen: Work) -> Work {
    if signal == Some(Work::Blocked) || screen == Work::Blocked {
        return Work::Blocked;
    }
    signal.unwrap_or(screen)
}

/// `role` → StopFailure error for every recorded role whose last
/// signal is a `StopFailure` — what `backlot status` and the
/// summary's `failed` show as `last turn failed: <error>`.
pub fn failed_roles(dir: &Path, state: &StudioState) -> BTreeMap<String, String> {
    state
        .roles
        .iter()
        .filter_map(|(role, r)| {
            let sid = r.session_id.as_deref()?;
            let s = last_work(&tail(&signals_path(dir, role)), sid)?;
            (s.event == "StopFailure").then(|| {
                (
                    role.clone(),
                    s.error.clone().unwrap_or_else(|| "unknown".into()),
                )
            })
        })
        .collect()
}

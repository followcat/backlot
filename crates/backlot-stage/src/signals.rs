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

/// `true` for a line that is (or was) a permission prompt: the
/// `PermissionRequest` itself or its `permission_prompt` chime.
impl Signal {
    pub(crate) fn asks(&self) -> bool {
        self.event == "PermissionRequest"
            || (self.event == "Notification"
                && self.notification_type.as_deref() == Some("permission_prompt"))
    }
}

/// The tail's first non-`Notification` line back from the end.
fn first_event(tail: &str, session_id: &str) -> Option<Signal> {
    tail.lines()
        .rev()
        .filter_map(|line| parse(line, session_id))
        .find(|s| s.event != "Notification")
}

/// The request the role is still waiting on: the first
/// non-`Notification` line back from the end, when it is an
/// unanswered `PermissionRequest`. A `permission_prompt`
/// notification is the same prompt's six-second chime, not a
/// change of prompt — any other event ends the request, and a
/// request whose `ts_ms` is in `answered` already got its key.
pub fn pending_request(
    tail: &str,
    session_id: &str,
    answered: &std::collections::BTreeSet<u64>,
) -> Option<Signal> {
    let s = first_event(tail, session_id)?;
    (s.event == "PermissionRequest" && !answered.contains(&s.ts_ms)).then_some(s)
}

/// The session's work state from its signals: the last mapped
/// line, except that a prompt line reads `Blocked` only while its
/// request is pending — Claude Code emits no hook between the
/// answer and the tool's own `PostToolUse`, so an answered
/// request reads `Working` until the next signal. A chime with no
/// request behind it — a sandbox prompt sends none — keeps its
/// own `Blocked`.
pub fn state(
    tail: &str,
    session_id: &str,
    answered: &std::collections::BTreeSet<u64>,
) -> Option<Work> {
    let s = last_work(tail, session_id)?;
    if !s.asks() {
        return s.work();
    }
    match first_event(tail, session_id) {
        Some(f) if f.event == "PermissionRequest" => Some(if answered.contains(&f.ts_ms) {
            Work::Working
        } else {
            Work::Blocked
        }),
        _ => s.work(),
    }
}

/// The `PermissionRequest`s the stage already answered for `role`:
/// every closed `permission prompt` attention that cleared
/// `sent <key>` and was bound to a request — its
/// `attention/<seq>.signal` names the request. Rebuilt from the
/// ledger each sample, so a stage restart does not raise the same
/// request again nor send its key twice.
pub fn answered_requests(
    dir: &Path,
    state: &StudioState,
    role: &str,
) -> std::collections::BTreeSet<u64> {
    state
        .attention
        .iter()
        .filter(|a| {
            a.role == role
                && !a.open
                && a.cleared_reason
                    .as_deref()
                    .is_some_and(|r| r.starts_with("sent "))
        })
        .filter_map(|a| {
            std::fs::read_to_string(dir.join("attention").join(format!("{}.signal", a.seq))).ok()
        })
        .filter_map(|s| serde_json::from_str::<Signal>(&s).ok())
        .map(|s| s.ts_ms)
        .collect()
}

/// The work-map answer from the three sources, in order: the
/// role's own hook signal (S9f), then the terminal's structured
/// state (S12d), then the stage's screen read. A `blocked` from
/// any source wins over everything else — a prompt on any channel
/// must still open an episode.
pub fn combine(signal: Option<Work>, structured: Option<Work>, screen: Work) -> Work {
    if [signal, structured, Some(screen)].contains(&Some(Work::Blocked)) {
        return Work::Blocked;
    }
    signal.or(structured).unwrap_or(screen)
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

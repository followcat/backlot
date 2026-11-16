//! The Claude Code layer-2 entry: `~/.claude/sessions/<pid>.json`
//! reports the process's own word, and `shell` means the turn is idle
//! while background tasks (shells, monitors, in-process sub agents)
//! still run — exactly Waiting. Verified against the shipped binary.
//!
//! Only `pid` and `status` are read: the file also carries socket
//! paths and names that are never stored, logged or shown.

/// The sessions file's verdict: it parses, its `pid` is the probed
/// process and its `status` is exactly `shell`. Every other content —
/// another pid, `busy`/`idle`/`waiting`, a missing field, unreadable
/// JSON — is not Waiting.
pub(crate) fn session_is_shell(text: &str, pid: i32) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    let file_pid = value.get("pid").and_then(|pid| {
        pid.as_i64()
            .or_else(|| pid.as_str().and_then(|digits| digits.parse().ok()))
    });
    file_pid == Some(i64::from(pid))
        && value.get("status").and_then(|status| status.as_str()) == Some("shell")
}

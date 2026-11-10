//! `backlot hook` — the command the per-session Claude settings
//! file points every hook event at (S9f). It reads the event's JSON
//! on stdin to the end and appends one JSON line to `--file`: the
//! fields the stage's signal read needs and nothing else — no
//! `transcript_path`, no `last_assistant_message`, no prompt text.
//!
//! It must never get in the runner's way: nothing on stdout or
//! stderr, exit 0 on every path — bad JSON, a missing directory, a
//! full disk, a closed stdin — and no network or hc call. The
//! append is one `write` of the line with `O_APPEND`, the parent
//! directory created if needed.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The most one tool_input string keeps — char-boundary cut. Tried
/// smaller until the whole line fits `LINE_CAP`.
const STRING_CAP: usize = 2000;
/// The written line's hard cap.
pub(crate) const LINE_CAP: usize = 4096;
/// The most stdin is read — a hook payload is small.
const STDIN_CAP: u64 = 1 << 20;

/// `hook --file <path>`: the `--file` value, or `None` — and a
/// missing one still exits 0, since a hook may not fail its runner.
fn file_arg(args: &[String]) -> Option<PathBuf> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--file" {
            return it.next().map(PathBuf::from);
        }
        if let Some(v) = a.strip_prefix("--file=") {
            return Some(PathBuf::from(v));
        }
    }
    None
}

/// `s` cut to at most `cap` bytes on a char boundary.
fn cut(s: &str, cap: usize) -> &str {
    if s.len() <= cap {
        return s;
    }
    let mut e = cap;
    while !s.is_char_boundary(e) {
        e -= 1;
    }
    &s[..e]
}

fn field<'a>(payload: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    payload.get(key).and_then(|v| v.as_str())
}

/// The trimmed `tool_input` a `PermissionRequest` carries: Bash's
/// `command` and `description`, or the file tool's `file_path`.
/// Anything else — a tool input without these fields — records
/// nothing; the tool name still says what was asked.
fn tool_input(payload: &serde_json::Value, cap: usize) -> Option<serde_json::Value> {
    let input = payload.get("tool_input")?;
    let mut out = serde_json::Map::new();
    if let Some(command) = field(input, "command") {
        out.insert("command".into(), cut(command, cap).into());
        if let Some(description) = field(input, "description") {
            out.insert("description".into(), cut(description, cap).into());
        }
    } else if let Some(path) = field(input, "file_path") {
        out.insert("file_path".into(), cut(path, cap).into());
    }
    (!out.is_empty()).then(|| out.into())
}

/// One signal line for the payload: `ts_ms`, the `hook_event_name`
/// as `event`, `session_id`, the notification type and
/// StopFailure's error type when present, and for a
/// `PermissionRequest` the tool name plus a trimmed `tool_input`.
fn line_for(payload: &serde_json::Value, ts_ms: u64, cap: usize) -> String {
    let mut obj = serde_json::Map::new();
    obj.insert("ts_ms".into(), ts_ms.into());
    let mut put = |key: &str, value: Option<&str>| {
        if let Some(v) = value {
            obj.insert(key.into(), cut(v, cap).into());
        }
    };
    put("event", field(payload, "hook_event_name"));
    put("session_id", field(payload, "session_id"));
    put("notification_type", field(payload, "notification_type"));
    // The real payload field is unverified (limits.md); read both
    // spellings.
    put(
        "error",
        field(payload, "error").or_else(|| field(payload, "error_type")),
    );
    if field(payload, "hook_event_name") == Some("PermissionRequest") {
        put("tool_name", field(payload, "tool_name"));
        if let Some(input) = tool_input(payload, cap) {
            obj.insert("tool_input".into(), input);
        }
    }
    let mut s = serde_json::to_string(&serde_json::Value::Object(obj)).unwrap_or_default();
    s.push('\n');
    s
}

/// One `write` of the line with `O_APPEND`, creating the parent
/// directory if needed; every failure is ignored.
fn append(path: &Path, line: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = f.write(line.as_bytes());
    }
}

/// The command the settings file runs. Always `ExitCode::SUCCESS`,
/// always silent: a hook may never get in the runner's way — its
/// stdout would land in the runner's context and a nonzero exit
/// would mark the event failed.
pub fn run(args: &[String]) -> ExitCode {
    run_io(args, &mut std::io::stdin().lock())
}

pub(crate) fn run_io(args: &[String], stdin: &mut impl Read) -> ExitCode {
    let Some(file) = file_arg(args) else {
        return ExitCode::SUCCESS;
    };
    let mut buf = Vec::new();
    if stdin.take(STDIN_CAP).read_to_end(&mut buf).is_err() {
        return ExitCode::SUCCESS;
    }
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&buf) else {
        return ExitCode::SUCCESS;
    };
    let ts_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    // The trimmed strings shrink until the line fits its cap.
    let line = [STRING_CAP, 512, 64]
        .into_iter()
        .map(|cap| line_for(&payload, ts_ms, cap))
        .find(|line| line.len() <= LINE_CAP)
        .unwrap_or_else(|| "{\"ts_ms\":0}\n".into());
    append(&file, &line);
    ExitCode::SUCCESS
}

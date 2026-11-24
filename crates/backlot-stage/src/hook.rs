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
/// Fail-open time budget for `agy` hook calls (under the 2 s limit).
const AGY_BUDGET: std::time::Duration = std::time::Duration::from_millis(1500);

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let eq = format!("{flag}=");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == flag {
            return it.next().map(String::as_str);
        }
        if let Some(v) = a.strip_prefix(&eq) {
            return Some(v);
        }
    }
    None
}

/// `hook --file <path>`: the `--file` value, or `None` — and a
/// missing one still exits 0, since a hook may not fail its runner.
fn file_arg(args: &[String]) -> Option<PathBuf> {
    flag_value(args, "--file").map(PathBuf::from)
}

fn runner_arg(args: &[String]) -> Option<&str> {
    flag_value(args, "--runner")
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
    } else if let Some(path) = field(input, "file_path").or_else(|| field(input, "path")) {
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

fn normalize_payload(runner: Option<&str>, payload: &serde_json::Value) -> serde_json::Value {
    if runner != Some("agy") {
        return payload.clone();
    }
    let mut obj = serde_json::Map::new();
    if let Some(ev) = field(payload, "hookEventName").or_else(|| field(payload, "hook_event_name"))
    {
        let mapped = match ev {
            "PreInvocation" => "UserPromptSubmit",
            "PostInvocation" => "Stop",
            other => other,
        };
        obj.insert("hook_event_name".into(), mapped.into());
    }
    if let Some(sid) = field(payload, "conversationId").or_else(|| field(payload, "session_id")) {
        obj.insert("session_id".into(), sid.into());
    }
    if let Some(nt) = field(payload, "notification_type") {
        obj.insert("notification_type".into(), nt.into());
    }
    if let Some(err) = field(payload, "error")
        .or_else(|| field(payload, "error_type"))
        .filter(|s| !s.is_empty())
    {
        obj.insert("error".into(), err.into());
    }
    serde_json::Value::Object(obj)
}

fn agy_run_command_line(payload: &serde_json::Value) -> Option<&str> {
    let tc = payload.get("toolCall")?;
    if tc.get("name").and_then(|v| v.as_str()) != Some("run_command") {
        return None;
    }
    let args = tc.get("args")?;
    args.get("CommandLine")
        .or_else(|| args.get("commandLine"))
        .or_else(|| args.get("command"))
        .and_then(|v| v.as_str())
}

fn agy_deny_json(command: &str) -> String {
    let reason =
        format!("refused by backlot: {command} is the person's to run; ask with backlot ask");
    serde_json::to_string(&serde_json::json!({
        "decision": "deny",
        "reason": reason,
    }))
    .unwrap_or_else(|_| r#"{"decision":"deny","reason":"refused by backlot"}"#.into())
}

fn line_with_cap(payload: &serde_json::Value, ts_ms: u64) -> String {
    // The trimmed strings shrink until the line fits its cap.
    [STRING_CAP, 512, 64]
        .into_iter()
        .map(|cap| line_for(payload, ts_ms, cap))
        .find(|line| line.len() <= LINE_CAP)
        .unwrap_or_else(|| "{\"ts_ms\":0}\n".into())
}

fn write_agy_allow(stdout: &mut impl Write) {
    let _ = writeln!(stdout, r#"{{"decision":"allow"}}"#);
    let _ = stdout.flush();
}

/// The command the settings file runs. Always `ExitCode::SUCCESS`,
/// silent on non-PreToolUse events (stdout carries `{"decision":...}`
/// only for `agy` `PreToolUse`).
pub fn run(args: &[String]) -> ExitCode {
    let env = crate::cli::Env::process();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut stdout = std::io::stdout().lock();
    run_io_env(
        args,
        &env,
        &cwd,
        AGY_BUDGET,
        &mut std::io::stdin().lock(),
        &mut stdout,
    )
}

#[cfg(test)]
pub(crate) fn run_io(args: &[String], stdin: &mut impl Read) -> ExitCode {
    let env = crate::cli::Env::default();
    let mut sink = std::io::sink();
    run_io_env(args, &env, Path::new("."), AGY_BUDGET, stdin, &mut sink)
}

pub(crate) fn run_io_env(
    args: &[String],
    env: &crate::cli::Env,
    cwd: &Path,
    budget: std::time::Duration,
    stdin: &mut impl Read,
    stdout: &mut impl Write,
) -> ExitCode {
    run_io_inner(args, env, cwd, budget, stdin, stdout, None)
}

#[cfg(test)]
pub(crate) fn run_io_with_delay(
    args: &[String],
    env: &crate::cli::Env,
    cwd: &Path,
    budget: std::time::Duration,
    delay: std::time::Duration,
    stdin: &mut impl Read,
    stdout: &mut impl Write,
) -> ExitCode {
    run_io_inner(args, env, cwd, budget, stdin, stdout, Some(delay))
}

fn run_io_inner(
    args: &[String],
    env: &crate::cli::Env,
    cwd: &Path,
    budget: std::time::Duration,
    stdin: &mut impl Read,
    stdout: &mut impl Write,
    test_delay: Option<std::time::Duration>,
) -> ExitCode {
    let runner = runner_arg(args);
    let is_agy = runner == Some("agy");
    let file = file_arg(args);
    if !is_agy && file.is_none() {
        return ExitCode::SUCCESS;
    }
    let mut buf = Vec::new();
    if stdin.take(STDIN_CAP).read_to_end(&mut buf).is_err() {
        if is_agy {
            write_agy_allow(stdout);
        }
        return ExitCode::SUCCESS;
    }
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&buf) else {
        if is_agy {
            write_agy_allow(stdout);
        }
        return ExitCode::SUCCESS;
    };
    let ts_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let normalized = normalize_payload(runner, &payload);
    let line = file.as_ref().map(|_| line_with_cap(&normalized, ts_ms));

    if !is_agy {
        if let (Some(f), Some(l)) = (file, line) {
            append(&f, &l);
        }
        return ExitCode::SUCCESS;
    }

    let is_pre_tool = field(&payload, "hookEventName") == Some("PreToolUse");
    let refused = if is_pre_tool {
        agy_run_command_line(&payload).and_then(crate::guard_rules::refused_command_line)
    } else {
        None
    };

    let worker_env = crate::cli::Env {
        vars: env.vars.clone(),
    };
    let worker_cwd = cwd.to_path_buf();
    let refused_for_worker = refused.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        if let Some(d) = test_delay {
            std::thread::sleep(d);
        }
        if let (Some(f), Some(l)) = (file, line) {
            append(&f, &l);
        }
        let outcome = match refused_for_worker {
            Some(ref cmd) => crate::guard::record_refusal(&worker_env, &worker_cwd, cmd),
            None => Ok(()),
        };
        let _ = tx.send(outcome);
    });

    if !is_pre_tool {
        let _ = rx.recv_timeout(budget);
        return ExitCode::SUCCESS;
    }

    match (refused, rx.recv_timeout(budget)) {
        (Some(cmd), Ok(Ok(()))) => {
            let _ = writeln!(stdout, "{}", agy_deny_json(&cmd));
            let _ = stdout.flush();
        }
        _ => write_agy_allow(stdout),
    }
    ExitCode::SUCCESS
}

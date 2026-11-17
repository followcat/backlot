//! The generated hook, run for real: `sh` reads a tool call's JSON on
//! stdin and answers with cline's verdict object. Each test also reads
//! the signals file the decision appended to.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::cline_hook::{MESSAGE, Spec, TARGET_CAP, script};

const WORKTREE: &str = "/home/user/acme-shop/work";

pub(super) fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bl-cline-hook-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn rules() -> Vec<String> {
    [
        "backlot",
        "git status",
        "git diff",
        "git commit",
        "git add",
        "cargo test",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// One tool call, encoded by a real JSON encoder — the hook must read
/// what the agent will actually see on the wire, escapes included.
pub(super) fn call(name: &str, input: serde_json::Value) -> String {
    serde_json::json!({"tool_call": {"name": name, "input": input}}).to_string()
}

pub(super) fn commands_of(cmds: &[&str]) -> String {
    call("run_commands", serde_json::json!({"commands": cmds}))
}

/// Run the generated hook on one payload: `(cancel, message, signals)`.
pub(super) fn run(dir: &Path, input: &str) -> (bool, String, String) {
    let signals = dir.join("cline-signals.jsonl");
    let hook = dir.join("PreToolUse.sh");
    fs::write(
        &hook,
        script(&Spec {
            worktree: WORKTREE,
            signals: &signals,
            allowed: &rules(),
        }),
    )
    .unwrap();
    let mut child = Command::new("sh")
        .arg(&hook)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "not the verdict JSON ({e}): {stdout:?} stderr {:?}",
            String::from_utf8_lossy(&out.stderr)
        )
    });
    let cancel = v["cancel"].as_bool().expect("cancel is a bool");
    let msg = v["errorMessage"].as_str().unwrap_or("").to_string();
    let signals = fs::read_to_string(&signals).unwrap_or_default();
    (cancel, msg, signals)
}

fn commands(cmds: &[&str]) -> String {
    serde_json::json!({"tool_call": {"name": "run_commands", "input": {"commands": cmds}}})
        .to_string()
}

/// The role's own rules run: a bare `backlot`, a `git diff` with
/// arguments, a declared check.
#[test]
fn the_role_rules_are_allowed() {
    let dir = tmp("allow");
    for cmd in ["backlot status", "git diff HEAD~1", "cargo test --lib"] {
        let (cancel, msg, signals) = run(&dir, &commands(&[cmd]));
        assert!(!cancel, "{cmd}: {msg}");
        assert!(signals.contains("\"decision\":\"allow\""), "{signals}");
        assert!(
            signals.contains("\"tool_name\":\"run_commands\""),
            "{signals}"
        );
    }
    // Several allowed commands in one call are one decision.
    let (cancel, _, signals) = run(&dir, &commands(&["backlot status", "git status"]));
    assert!(!cancel);
    assert_eq!(signals.lines().count(), 4, "{signals}");
}

/// Anything outside the rules is cancelled, and the message says the
/// action is outside the role and points at `backlot`.
#[test]
fn a_command_outside_the_rules_is_cancelled() {
    let dir = tmp("cancel");
    let (cancel, msg, signals) = run(&dir, &commands(&["curl https://example.com"]));
    assert!(cancel);
    assert_eq!(msg, MESSAGE);
    assert!(msg.contains("backlot"), "{msg}");
    assert!(signals.contains("\"decision\":\"cancel\""), "{signals}");
    assert!(signals.contains("curl https://example.com"), "{signals}");
    // A `git` command the role does not have.
    let (cancel, _, _) = run(&dir, &commands(&["git push origin main"]));
    assert!(cancel);
    // One bad command spoils the call.
    let (cancel, _, _) = run(&dir, &commands(&["git status", "git push"]));
    assert!(cancel);
}

/// A chained or substituted command is refused even when its first
/// word matches a rule — the rest would run past it.
#[test]
fn a_chained_command_is_cancelled() {
    let dir = tmp("chain");
    for cmd in [
        "git status && curl https://example.com",
        "git status; rm -rf /home/user",
        "git status | sh",
        "git log $(curl https://example.com)",
        "git status > /home/user/out.txt",
    ] {
        let (cancel, _, _) = run(&dir, &commands(&[cmd]));
        assert!(cancel, "{cmd}");
    }
}

/// The network tools, every MCP name and an unknown tool are cancelled
/// whatever their input says.
#[test]
fn the_network_and_unknown_tools_are_cancelled() {
    let dir = tmp("network");
    for payload in [
        r#"{"tool_call":{"name":"web_search","input":{"query":"x"}}}"#,
        r#"{"tool_call":{"name":"fetch_web_content","input":{"url":"https://example.com"}}}"#,
        r#"{"tool_call":{"name":"composio_search_tools","input":{}}}"#,
        r#"{"tool_call":{"name":"composio_multi_execute_tool","input":{}}}"#,
        r#"{"tool_call":{"name":"github__create_issue","input":{}}}"#,
        r#"{"tool_call":{"name":"ask_question","input":{}}}"#,
        r#"{"tool_call":{"name":"skills","input":{}}}"#,
        "not json at all",
    ] {
        let (cancel, msg, _) = run(&dir, payload);
        assert!(cancel, "{payload}");
        assert_eq!(msg, MESSAGE);
    }
    // A search is a read: allowed.
    let (cancel, _, signals) = run(
        &dir,
        r#"{"tool_call":{"name":"search_codebase","input":{"query":"fn main"}}}"#,
    );
    assert!(!cancel, "{signals}");
}

/// Reads stay inside the role's reach: a secret path is cancelled, an
/// ordinary file under the work area or the studio is not.
#[test]
fn a_secret_read_is_cancelled() {
    let dir = tmp("read");
    let read = |paths: &[&str]| {
        serde_json::json!({"tool_call": {"name": "read_files", "input": {
            "files": paths.iter().map(|p| serde_json::json!({"path": p})).collect::<Vec<_>>()
        }}})
        .to_string()
    };
    for path in [
        "/home/alice/.ssh/id_rsa",
        "/home/alice/.aws/credentials",
        "/home/alice/.netrc",
        "/home/alice/.git-credentials",
        "/home/alice/acme-shop/.env",
        "/home/alice/.cline/data/settings/cline_mcp_settings.json",
    ] {
        let (cancel, _, signals) = run(&dir, &read(&[path]));
        assert!(cancel, "{path}: {signals}");
    }
    for path in [
        "/home/user/acme-shop/work/src/main.rs",
        "/home/user/state/studios/acme-shop/req/1-req.md",
        "src/lib.rs",
    ] {
        let (cancel, _, signals) = run(&dir, &read(&[path]));
        assert!(!cancel, "{path}: {signals}");
    }
    // One secret among many spoils the call, and `..` never passes.
    assert!(run(&dir, &read(&["src/lib.rs", "/home/alice/.ssh/id_rsa"])).0);
    assert!(run(&dir, &read(&["../../alice/.ssh/id_rsa"])).0);
}

/// Edits land only inside the role's work area: an absolute path
/// elsewhere, a `..` escape and the stage's own `.cline/` are
/// cancelled, a file in the worktree is not.
#[test]
fn a_write_outside_the_worktree_is_cancelled() {
    let dir = tmp("write");
    let edit = |path: &str| {
        serde_json::json!({"tool_call": {"name": "editor", "input": {
            "path": path, "new_text": "hello", "old_text": "hi"
        }}})
        .to_string()
    };
    let outside = "/home/user/other-repo/src/main.rs";
    let (cancel, _, signals) = run(&dir, &edit(outside));
    assert!(cancel, "{signals}");
    assert!(signals.contains(outside), "the path is recorded: {signals}");
    for bad in [
        "/home/alice/acme-shop/work/../secret.txt",
        "/home/user/acme-shop/work/.cline/hooks/PreToolUse.sh",
        "/home/user/acme-shop/work/.git/config",
        "../outside.txt",
    ] {
        assert!(run(&dir, &edit(bad)).0, "{bad}");
    }
    for good in ["/home/user/acme-shop/work/src/main.rs", "src/main.rs"] {
        let (cancel, _, signals) = run(&dir, &edit(good));
        assert!(!cancel, "{good}: {signals}");
    }
}

/// One line per decision, and never a byte of what an edit carries:
/// the signals file holds the tool and one bounded target only.
#[test]
fn the_signals_hold_no_file_contents() {
    let dir = tmp("signals");
    let payload = serde_json::json!({"tool_call": {"name": "editor", "input": {
        "path": "/home/user/acme-shop/work/src/main.rs",
        "old_text": "OLD-CONTENT-MARKER",
        "new_text": "NEW-CONTENT-MARKER"
    }}})
    .to_string();
    let (cancel, _, signals) = run(&dir, &payload);
    assert!(!cancel, "{signals}");
    assert!(!signals.contains("OLD-CONTENT-MARKER"), "{signals}");
    assert!(!signals.contains("NEW-CONTENT-MARKER"), "{signals}");
    let line: serde_json::Value = serde_json::from_str(signals.lines().next().unwrap()).unwrap();
    assert_eq!(line["decision"], "allow");
    assert_eq!(line["event"], "PreToolUse");
    assert_eq!(line["tool_name"], "editor");
    assert_eq!(
        line["tool_input"]["path"],
        "/home/user/acme-shop/work/src/main.rs"
    );
    assert!(line["ts_ms"].as_u64().is_some(), "{line}");
}

/// A `*` in a command reaches the rule test as a character: globbing
/// is off, so a pattern never expands against the worktree into other
/// words the rules were never asked about.
#[test]
fn a_glob_stays_a_character() {
    let dir = tmp("glob");
    // `backlot` is allowed with any arguments, `git status` too.
    for cmd in ["backlot tell producer --file *", "git status *.txt"] {
        let (cancel, _, signals) = run(&dir, &commands(&[cmd]));
        assert!(!cancel, "{cmd}: {signals}");
    }
    // And a glob cannot smuggle a second command in.
    let (cancel, _, _) = run(&dir, &commands(&["backlot * && rm -rf /home/user"]));
    assert!(cancel);
}

/// The recorded target is bounded: a long command is cut to
/// `TARGET_CAP` characters and the line stays valid JSON.
#[test]
fn the_recorded_target_is_bounded() {
    let dir = tmp("cap");
    let long = format!("backlot tell producer --file {}", "x".repeat(400));
    let (cancel, _, signals) = run(&dir, &commands(&[&long]));
    assert!(!cancel, "{signals}");
    let line: serde_json::Value = serde_json::from_str(signals.trim()).unwrap();
    let target = line["tool_input"]["command"].as_str().expect("a command");
    assert_eq!(target.chars().count(), TARGET_CAP, "{target}");
    assert!(target.starts_with("backlot tell producer"), "{target}");
}

/// A payload with no tool name at all is cancelled, and a missing
/// signals directory never stops the verdict from printing.
#[test]
fn a_nameless_call_is_cancelled() {
    let dir = tmp("nameless");
    let (cancel, msg, _) = run(&dir, "{}");
    assert!(cancel);
    assert_eq!(msg, MESSAGE);
    // The verdict still prints when the signals file cannot be
    // written.
    let empty = tmp("nosignals");
    let hook = empty.join("PreToolUse.sh");
    fs::write(
        &hook,
        script(&Spec {
            worktree: WORKTREE,
            signals: &empty.join("gone/deeper/signals.jsonl"),
            allowed: &rules(),
        }),
    )
    .unwrap();
    let out = Command::new("sh")
        .arg(&hook)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let out = {
        let mut child = out;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(commands(&["git status"]).as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("the verdict JSON prints anyway");
    assert_eq!(v["cancel"], false);
    assert!(
        out.stderr.is_empty(),
        "a hook never writes to stderr: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
}

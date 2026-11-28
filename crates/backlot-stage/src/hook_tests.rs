//! `backlot hook` (S9f): the written line's shape and caps, and
//! every failure path exiting 0 — bad JSON, a missing `--file`,
//! closed or failing stdin, an unwritable file.

use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};

use crate::hook::{LINE_CAP, run_io};
use crate::test_support::TestDir;

fn tmp() -> TestDir {
    TestDir::new("backlot-hook-test")
}

fn args(file: &Path) -> Vec<String> {
    vec!["--file".into(), file.to_string_lossy().into_owned()]
}

fn run(argv: &[String], stdin: &[u8]) -> ExitCode {
    run_io(argv, &mut &*stdin)
}

fn lines(file: &Path) -> Vec<Value> {
    std::fs::read_to_string(file)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn a_permission_request_writes_one_line() {
    let dir = tmp();
    let file = dir.join("signals.jsonl");
    let payload = json!({
        "session_id": "s-1",
        "hook_event_name": "PermissionRequest",
        "tool_name": "Bash",
        "tool_input": {"command": "cargo test", "description": "run the tests"},
        "transcript_path": "/secret/transcript",
        "cwd": "/home/user/acme-shop",
    });
    assert_eq!(
        run(&args(&file), payload.to_string().as_bytes()),
        ExitCode::SUCCESS
    );
    let got = lines(&file);
    let [line] = &got[..] else {
        panic!("want one line")
    };
    assert!(line["ts_ms"].as_u64().unwrap() > 0);
    assert_eq!(line["event"], "PermissionRequest");
    assert_eq!(line["session_id"], "s-1");
    assert_eq!(line["tool_name"], "Bash");
    assert_eq!(
        line["tool_input"],
        json!({"command": "cargo test", "description": "run the tests"})
    );
    // Nothing extra: no transcript_path, no cwd, no prompt.
    let keys: Vec<&String> = line.as_object().unwrap().keys().collect();
    assert_eq!(keys.len(), 5, "{keys:?}");
}

#[test]
fn a_file_tool_keeps_its_path_and_other_tools_keep_none() {
    let dir = tmp();
    let file = dir.join("signals.jsonl");
    for (name, input, want) in [
        (
            "Write",
            json!({"file_path": "/home/user/a.rs", "content": "long"}),
            json!({"file_path": "/home/user/a.rs"}),
        ),
        ("WebFetch", json!({"url": "http://x"}), Value::Null),
    ] {
        let payload = json!({
            "session_id": "s-1", "hook_event_name": "PermissionRequest",
            "tool_name": name, "tool_input": input,
        });
        assert_eq!(
            run(&args(&file), payload.to_string().as_bytes()),
            ExitCode::SUCCESS
        );
        let got = lines(&file);
        assert_eq!(got.last().unwrap()["tool_input"], want, "{name}");
    }
}

#[test]
fn notification_and_stop_failure_carry_their_kinds() {
    let dir = tmp();
    let file = dir.join("signals.jsonl");
    for payload in [
        json!({"session_id": "s-1", "hook_event_name": "Notification",
               "notification_type": "permission_prompt"}),
        json!({"session_id": "s-1", "hook_event_name": "StopFailure",
               "error": "rate_limit"}),
        json!({"session_id": "s-1", "hook_event_name": "Stop"}),
    ] {
        assert_eq!(
            run(&args(&file), payload.to_string().as_bytes()),
            ExitCode::SUCCESS
        );
    }
    let lines = lines(&file);
    assert_eq!(lines[0]["notification_type"], "permission_prompt");
    assert_eq!(lines[1]["error"], "rate_limit");
    assert!(lines[2].get("notification_type").is_none());
    assert!(lines[2].get("error").is_none());
}

#[test]
fn strings_and_the_line_stay_under_their_caps() {
    let dir = tmp();
    let file = dir.join("signals.jsonl");
    // A 3499-byte command whose byte 2000 splits an `é`: the
    // char-boundary cut gives the 1999 `x`s and the whole line fits
    // on the first pass.
    let command = format!("{}{}", "x".repeat(1999), "é".repeat(1500));
    let payload = json!({
        "session_id": "s-1", "hook_event_name": "PermissionRequest",
        "tool_name": "Bash",
        "tool_input": {"command": command, "description": "go"},
    });
    assert_eq!(
        run(&args(&file), payload.to_string().as_bytes()),
        ExitCode::SUCCESS
    );
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.len() <= LINE_CAP, "{}", text.len());
    let line: Value = serde_json::from_str(text.trim()).unwrap();
    let command = line["tool_input"]["command"].as_str().unwrap();
    assert_eq!(command, "x".repeat(1999).as_str());
    // Both strings at the 2000 cap would overflow the line: a
    // second pass shrinks them until it fits.
    let payload = json!({
        "session_id": "s-1", "hook_event_name": "PermissionRequest",
        "tool_name": "Bash",
        "tool_input": {"command": "c".repeat(3000), "description": "d".repeat(3000)},
    });
    assert_eq!(
        run(&args(&file), payload.to_string().as_bytes()),
        ExitCode::SUCCESS
    );
    let text = std::fs::read_to_string(&file).unwrap();
    let line: Value = serde_json::from_str(text.trim().lines().last().unwrap()).unwrap();
    assert!(text.lines().last().unwrap().len() <= LINE_CAP);
    assert!(line["tool_input"]["command"].as_str().unwrap().len() <= 2000);
    assert!(line["tool_input"]["description"].as_str().unwrap().len() <= 2000);
}

#[test]
fn every_failure_path_exits_zero_and_silent() {
    let dir = tmp();
    let file = dir.join("signals.jsonl");
    // Bad JSON — nothing written.
    assert_eq!(run(&args(&file), b"not json{"), ExitCode::SUCCESS);
    assert!(!file.exists());
    // Closed stdin.
    assert_eq!(run(&args(&file), b""), ExitCode::SUCCESS);
    assert!(!file.exists());
    // No --file at all.
    assert_eq!(run(&[], b"{}"), ExitCode::SUCCESS);
    assert_eq!(run(&["--file".into()], b"{}"), ExitCode::SUCCESS);
    // A failing reader.
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("closed"))
        }
    }
    assert_eq!(run_io(&args(&file), &mut Broken), ExitCode::SUCCESS);
    // An unwritable target: its parent path is a file.
    let blocker = dir.join("a-file");
    std::fs::write(&blocker, b"x").unwrap();
    let unreachable = blocker.join("sub").join("signals.jsonl");
    let payload = json!({"session_id": "s-1"});
    assert_eq!(
        run(&args(&unreachable), payload.to_string().as_bytes()),
        ExitCode::SUCCESS
    );
    assert!(!unreachable.exists());
}

#[test]
fn the_parent_directory_is_created() {
    let dir = tmp();
    let file = dir.join("deep").join("nested").join("signals.jsonl");
    let payload = json!({"session_id": "s-9", "hook_event_name": "Stop"});
    assert_eq!(
        run(&args(&file), payload.to_string().as_bytes()),
        ExitCode::SUCCESS
    );
    let got = lines(&file);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0]["event"], "Stop");
    // A second event appends — one line per event.
    let payload = json!({"session_id": "s-9", "hook_event_name": "UserPromptSubmit"});
    assert_eq!(
        run(&args(&file), payload.to_string().as_bytes()),
        ExitCode::SUCCESS
    );
    assert_eq!(lines(&file).len(), 2);
}

/// Non-object JSON — a bare string — still exits 0 and records a
/// line with just `ts_ms`.
#[test]
fn a_scalar_payload_writes_a_ts_line() {
    let dir = tmp();
    let file = dir.join("signals.jsonl");
    assert_eq!(run(&args(&file), b"\"hi\""), ExitCode::SUCCESS);
    let got = lines(&file);
    assert_eq!(got.len(), 1);
    assert!(got[0]["ts_ms"].as_u64().is_some());
    assert!(got[0].get("event").is_none());
}

fn role_fixture() -> (
    TestDir,
    crate::cli::Env,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    let dir = tmp();
    let state = dir.join("state");
    let studio = state.join("studios/acme");
    let work = studio.join("work/integration");
    let home = dir.join("home");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let mut env = crate::cli::Env::default();
    env.vars.insert(
        "BACKLOT_STATE_DIR".into(),
        state.to_string_lossy().into_owned(),
    );
    env.vars.insert("BACKLOT_STUDIO".into(), "acme".into());
    env.vars.insert("BACKLOT_ROLE".into(), "director".into());
    env.vars
        .insert("HOME".into(), home.to_string_lossy().into_owned());
    (dir, env, work, studio.join("roles/director/signals.jsonl"))
}

fn hook_roundtrip(runner: &str, payload: Value) -> (TestDir, std::path::PathBuf, String) {
    let (dir, env, work, signals) = role_fixture();
    let argv = vec![
        "--runner".into(),
        runner.into(),
        "--file".into(),
        signals.to_string_lossy().into_owned(),
    ];
    let mut out = Vec::new();
    let input = payload.to_string();
    let mut input = input.as_bytes();
    let code = crate::hook::run_io_env(
        &argv,
        &env,
        &work,
        std::time::Duration::from_millis(1500),
        &mut input,
        &mut out,
    );
    assert_eq!(code, ExitCode::SUCCESS);
    (dir, signals, String::from_utf8(out).unwrap())
}

#[test]
fn read_only_pre_tool_returns_each_runners_allow() {
    for (runner, payload, want) in [
        (
            "claude",
            json!({"hook_event_name":"PreToolUse","tool_input":{"command":"ls"}}),
            "permissionDecision",
        ),
        (
            "cmd",
            json!({"hook_event_name":"PreToolUse","tool_input":{"command":"ls"}}),
            "permissionDecision",
        ),
        (
            "devin",
            json!({"hook_event_name":"PermissionRequest","tool_input":{"command":"ls"}}),
            "\"decision\":\"approve\"",
        ),
        (
            "cursor",
            json!({"hook_event_name":"beforeShellExecution","command":"ls"}),
            "\"permission\":\"allow\"",
        ),
        (
            "agy",
            json!({"hookEventName":"PreToolUse","toolCall":{"name":"run_command","args":{"CommandLine":"ls"}}}),
            "\"decision\":\"allow\"",
        ),
    ] {
        let (_dir, signals, out) = hook_roundtrip(runner, payload);
        assert!(out.contains(want), "{runner}: {out}");
        let text = std::fs::read_to_string(signals).unwrap();
        assert!(text.contains("PermitAutoAllow"), "{runner}: {text}");
        assert!(text.contains("provably read-only"), "{runner}: {text}");
    }
}

#[test]
fn a_secret_readonly_shape_gets_no_hook_allow() {
    let (_dir, signals, out) = hook_roundtrip(
        "claude",
        json!({"hook_event_name":"PreToolUse","tool_input":{"command":"cat .env"}}),
    );
    assert!(out.is_empty(), "{out}");
    let text = std::fs::read_to_string(signals).unwrap();
    assert!(!text.contains("PermitAutoAllow"), "{text}");
}

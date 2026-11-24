use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use backlot_model::summary::Work;

use super::PrepNote;
use super::devin::{DevinPrep, config_content, prepare};
use crate::cli::Env;
use crate::hook::run_io_env;
use crate::signals;

fn tmp(label: &str) -> crate::test_support::TestDir {
    crate::test_support::TestDir::new(&format!("backlot-devin-{label}"))
}

fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=alice", "-c", "user.email=alice@host-a"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn run_hook(runner: &str, sig: &Path, body: &str) {
    let args = vec![
        "--runner".into(),
        runner.into(),
        "--file".into(),
        sig.to_string_lossy().into_owned(),
    ];
    let mut out = Vec::new();
    let code = run_io_env(
        &args,
        &Env::default(),
        Path::new("."),
        Duration::from_millis(1500),
        &mut Cursor::new(body.as_bytes()),
        &mut out,
    );
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(out.is_empty());
}

#[test]
fn cmd_and_devin_payloads_normalize_to_signals() {
    let dir = tmp("signals");

    let cmd_sig = dir.join("cmd-signals.jsonl");
    run_hook(
        "cmd",
        &cmd_sig,
        r#"{"session_id":"c-1","hook_event_name":"SessionStart"}"#,
    );
    assert_eq!(
        signals::last_work(&fs::read_to_string(&cmd_sig).unwrap(), "c-1"),
        None
    );
    run_hook(
        "cmd",
        &cmd_sig,
        r#"{"session_id":"c-1","hook_event_name":"PreToolUse","tool_name":"shell_command"}"#,
    );
    assert_eq!(
        signals::last_work(&fs::read_to_string(&cmd_sig).unwrap(), "c-1").and_then(|s| s.work()),
        Some(Work::Working)
    );
    run_hook(
        "cmd",
        &cmd_sig,
        r#"{"session_id":"c-1","hook_event_name":"Stop"}"#,
    );
    assert_eq!(
        signals::last_work(&fs::read_to_string(&cmd_sig).unwrap(), "c-1").and_then(|s| s.work()),
        Some(Work::Waiting)
    );

    let devin_sig = dir.join("devin-signals.jsonl");
    run_hook(
        "devin",
        &devin_sig,
        r#"{"session_id":"lilac-kite","hook_event_name":"UserPromptSubmit"}"#,
    );
    assert_eq!(
        signals::last_work(&fs::read_to_string(&devin_sig).unwrap(), "lilac-kite")
            .and_then(|s| s.work()),
        Some(Work::Working)
    );
    run_hook(
        "devin",
        &devin_sig,
        r#"{"session_id":"lilac-kite","hook_event_name":"PermissionRequest","tool_name":"exec","tool_input":{"command":"touch x"}}"#,
    );
    let tail = fs::read_to_string(&devin_sig).unwrap();
    let last = signals::last_work(&tail, "lilac-kite").unwrap();
    assert_eq!(last.work(), Some(Work::Blocked));
    assert_eq!(last.tool_name.as_deref(), Some("exec"));
    assert_eq!(last.tool_input.as_ref().unwrap()["command"], "touch x");
    run_hook(
        "devin",
        &devin_sig,
        r#"{"session_id":"lilac-kite","hook_event_name":"Stop"}"#,
    );
    assert_eq!(
        signals::last_work(&fs::read_to_string(&devin_sig).unwrap(), "lilac-kite")
            .and_then(|s| s.work()),
        Some(Work::Waiting)
    );
}

#[test]
fn devin_config_splices_byte_exact_excludes_new_and_skips_tracked() {
    let dir = tmp("prep");
    let sig = dir.join("signals.jsonl");
    let existing =
        "{\n  \"permissions\": {\n    \"allow\": [\n      \"Bash(echo *)\"\n    ]\n  }\n}\n";
    let spliced = String::from_utf8(config_content(Some(existing.as_bytes()), &sig)).unwrap();
    assert!(
        spliced.starts_with(
            "{\n  \"permissions\": {\n    \"allow\": [\n      \"Bash(echo *)\"\n    ]\n  }"
        ),
        "{spliced}"
    );
    let parsed: serde_json::Value = serde_json::from_str(&spliced).unwrap();
    assert_eq!(parsed["hooks"]["PermissionRequest"][0]["matcher"], "");
    assert!(
        parsed["hooks"]["Stop"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("hook --runner devin --file")
    );

    let repo = dir.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("f"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);

    let prep = DevinPrep {
        config: repo.join(".devin/config.local.json"),
        signals: sig.clone(),
    };
    assert_eq!(prepare(&prep).unwrap(), None);
    let exclude = fs::read_to_string(repo.join(".git/info/exclude")).unwrap();
    assert!(
        exclude
            .lines()
            .any(|l| l.trim() == ".devin/config.local.json"),
        "{exclude}"
    );

    let tracked = dir.join("tracked");
    fs::create_dir_all(tracked.join(".devin")).unwrap();
    git(&tracked, &["init", "-b", "main"]);
    let cfg = tracked.join(".devin/config.local.json");
    let orig = "{\"custom\":true}\n";
    fs::write(&cfg, orig).unwrap();
    git(&tracked, &["add", ".devin/config.local.json"]);
    git(&tracked, &["commit", "-m", "track devin config"]);
    let tprep = DevinPrep {
        config: cfg.clone(),
        signals: sig,
    };
    assert_eq!(
        prepare(&tprep).unwrap(),
        Some(PrepNote::HooksUnavailable(".devin/config.local.json"))
    );
    assert_eq!(fs::read_to_string(&cfg).unwrap(), orig);
}

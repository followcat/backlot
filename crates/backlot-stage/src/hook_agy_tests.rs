use std::io::Cursor;
use std::process::ExitCode;
use std::time::Duration;

use backlot_model::event::{EventKind, OUTWARD_ACTION};
use backlot_model::summary::Work;

use crate::cli::Env;
use crate::cli_testkit::{CAST_TOML, fixture, ok, scratch};
use crate::hook::{run_io_env, run_io_with_delay};
use crate::signals;
use crate::store::{load_events, studio_dir};

fn run_agy(args: &[String], env: &Env, cwd: &std::path::Path, body: &str) -> String {
    let mut out = Vec::new();
    let code = run_io_env(
        args,
        env,
        cwd,
        Duration::from_millis(1500),
        &mut Cursor::new(body.as_bytes()),
        &mut out,
    );
    assert_eq!(code, ExitCode::SUCCESS);
    String::from_utf8(out).unwrap()
}

fn last_work(sig: &std::path::Path, session_id: &str) -> Option<signals::Signal> {
    let tail = std::fs::read_to_string(sig).unwrap_or_default();
    signals::last_work(&tail, session_id)
}

#[test]
fn agy_normalization_maps_events_and_filters_by_conversation() {
    let (s, env) = scratch();
    let sig = s.files.join("signals.jsonl");
    let args = vec![
        "--runner".into(),
        "agy".into(),
        "--file".into(),
        sig.to_string_lossy().into_owned(),
    ];

    let out = run_agy(
        &args,
        &env,
        &s.files,
        r#"{"conversationId":"conv-1","hookEventName":"SessionStart","cwd":"/home/user/repo"}"#,
    );
    assert!(out.is_empty());
    assert_eq!(last_work(&sig, "conv-1"), None);

    let out = run_agy(
        &args,
        &env,
        &s.files,
        r#"{"conversationId":"conv-1","hookEventName":"PreInvocation","cwd":"/home/user/repo"}"#,
    );
    assert!(out.is_empty());
    assert_eq!(
        last_work(&sig, "conv-1").and_then(|w| w.work()),
        Some(Work::Working)
    );

    // Another conversation's Stop must not overwrite conv-1's Working state.
    let out = run_agy(
        &args,
        &env,
        &s.files,
        r#"{"conversationId":"conv-2","hookEventName":"PostInvocation","cwd":"/home/user/repo"}"#,
    );
    assert!(out.is_empty());
    assert_eq!(
        last_work(&sig, "conv-1").and_then(|w| w.work()),
        Some(Work::Working)
    );
    assert_eq!(
        last_work(&sig, "conv-2").and_then(|w| w.work()),
        Some(Work::Waiting)
    );

    let out = run_agy(
        &args,
        &env,
        &s.files,
        r#"{"conversationId":"conv-1","hookEventName":"PostToolUse","cwd":"/home/user/repo"}"#,
    );
    assert!(out.is_empty());
    assert_eq!(
        last_work(&sig, "conv-1").and_then(|w| w.work()),
        Some(Work::Working)
    );

    // Empty error stays None; non-empty error is preserved alongside Waiting.
    let out = run_agy(
        &args,
        &env,
        &s.files,
        r#"{"conversationId":"conv-1","hookEventName":"PostInvocation","error":""}"#,
    );
    assert!(out.is_empty());
    let w_clean = last_work(&sig, "conv-1").unwrap();
    assert_eq!(w_clean.work(), Some(Work::Waiting));
    assert_eq!(w_clean.error, None);

    let out = run_agy(
        &args,
        &env,
        &s.files,
        r#"{"conversationId":"conv-1","hookEventName":"Stop","error":"rate limit"}"#,
    );
    assert!(out.is_empty());
    let w_err = last_work(&sig, "conv-1").unwrap();
    assert_eq!(w_err.work(), Some(Work::Waiting));
    assert_eq!(w_err.error.as_deref(), Some("rate limit"));
}

#[test]
fn agy_pre_tool_use_refuses_outward_command_records_once_and_allows_safe_calls() {
    let (s, mut env) = scratch();
    let cast = fixture(&s, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    env.vars.insert("BACKLOT_ROLE".into(), "director".into());

    let sig = s.files.join("signals.jsonl");
    let args = vec![
        "--runner".into(),
        "agy".into(),
        "--file".into(),
        sig.to_string_lossy().into_owned(),
    ];

    let allowed = run_agy(
        &args,
        &env,
        &s.files,
        r#"{"conversationId":"conv-1","hookEventName":"PreToolUse","toolCall":{"name":"run_command","args":{"CommandLine":"git status"}}}"#,
    );
    let parsed_allow: serde_json::Value = serde_json::from_str(allowed.trim()).unwrap();
    assert_eq!(parsed_allow["decision"], "allow");
    assert_eq!(
        last_work(&sig, "conv-1").and_then(|w| w.work()),
        Some(Work::Working)
    );

    let push_payload = r#"{"conversationId":"conv-1","hookEventName":"PreToolUse","toolCall":{"name":"run_command","args":{"CommandLine":"git push origin HEAD"}}}"#;
    let denied1 = run_agy(&args, &env, &s.files, push_payload);
    let parsed1: serde_json::Value = serde_json::from_str(denied1.trim()).unwrap();
    assert_eq!(parsed1["decision"], "deny");
    assert_eq!(
        parsed1["reason"],
        "refused by backlot: git push origin is the person's to run; ask with backlot ask"
    );

    // Second refusal in the same episode records role.refused again, attention stays once.
    let denied2 = run_agy(&args, &env, &s.files, push_payload);
    let parsed2: serde_json::Value = serde_json::from_str(denied2.trim()).unwrap();
    assert_eq!(parsed2["decision"], "deny");

    let (events, _) = load_events(&studio_dir(&s.dir, "acme-shop")).unwrap();
    let refused_count = events
        .iter()
        .filter(|e| matches!(&e.kind, EventKind::RoleRefused { role, command, .. } if role == "director" && command == "git push origin"))
        .count();
    let attn_count = events
        .iter()
        .filter(|e| matches!(&e.kind, EventKind::AttentionRaised { role, reason, .. } if role == "director" && reason == OUTWARD_ACTION))
        .count();
    assert_eq!(refused_count, 2);
    assert_eq!(attn_count, 1);
}

#[test]
fn agy_pre_tool_use_fails_open_on_internal_error_invalid_json_and_timeout() {
    let (s, mut env) = scratch();
    let studio = studio_dir(&s.dir, "acme-shop");
    std::fs::create_dir_all(&studio).unwrap();
    // Corrupt events.jsonl makes record_refusal return Err(...)
    std::fs::write(studio.join("events.jsonl"), "{not-valid-json\n").unwrap();
    env.vars.insert("BACKLOT_ROLE".into(), "director".into());

    let sig = s.files.join("signals.jsonl");
    let args = vec![
        "--runner".into(),
        "agy".into(),
        "--file".into(),
        sig.to_string_lossy().into_owned(),
    ];
    let push_payload = r#"{"conversationId":"conv-1","hookEventName":"PreToolUse","toolCall":{"name":"run_command","args":{"CommandLine":"git push origin HEAD"}}}"#;

    let on_err = run_agy(&args, &env, &s.files, push_payload);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(on_err.trim()).unwrap()["decision"],
        "allow"
    );

    let on_bad_json = run_agy(&args, &env, &s.files, "{broken");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(on_bad_json.trim()).unwrap()["decision"],
        "allow"
    );

    let mut out = Vec::new();
    let code = run_io_with_delay(
        &args,
        &env,
        &s.files,
        Duration::from_millis(20),
        Duration::from_millis(120),
        &mut Cursor::new(push_payload.as_bytes()),
        &mut out,
    );
    assert_eq!(code, ExitCode::SUCCESS);
    let timed_out = String::from_utf8(out).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(timed_out.trim()).unwrap()["decision"],
        "allow"
    );
}

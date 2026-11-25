use std::io::Cursor;
use std::process::ExitCode;
use std::time::Duration;

use backlot_model::event::{EventKind, OUTWARD_ACTION, RefusalDisposition};
use backlot_model::summary::Work;

use crate::cli::Env;
use crate::cli_testkit::{CAST_TOML, fixture, ok, scratch};
use crate::hook::run_io_with_delay;
use crate::signals;
use crate::store::{load_events, studio_dir};

fn run_agy(args: &[String], env: &Env, cwd: &std::path::Path, body: &str) -> String {
    let mut out = Vec::new();
    let (code, done) = run_io_with_delay(
        args,
        env,
        cwd,
        Duration::from_millis(1500),
        None,
        &mut Cursor::new(body.as_bytes()),
        &mut out,
    );
    done.recv_timeout(Duration::from_secs(5)).unwrap();
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
        "refused by backlot (unrecorded remote URL 'origin'): sent to the person as a card; wait for the result"
    );

    let force_payload = r#"{"conversationId":"conv-1","hookEventName":"PreToolUse","toolCall":{"name":"run_command","args":{"CommandLine":"git push --force origin HEAD"}}}"#;
    let denied2 = run_agy(&args, &env, &s.files, force_payload);
    let parsed2: serde_json::Value = serde_json::from_str(denied2.trim()).unwrap();
    assert_eq!(parsed2["decision"], "deny");
    let denied3 = run_agy(&args, &env, &s.files, force_payload);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(denied3.trim()).unwrap()["decision"],
        "deny"
    );

    let (events, _) = load_events(&studio_dir(&s.dir, "acme-shop")).unwrap();
    let card_count = events
        .iter()
        .filter(|e| matches!(&e.kind, EventKind::RoleRefused { role, disposition: RefusalDisposition::Card, .. } if role == "director"))
        .count();
    let refused_count = events
        .iter()
        .filter(|e| matches!(&e.kind, EventKind::RoleRefused { role, disposition: RefusalDisposition::Refused, .. } if role == "director"))
        .count();
    let attn_count = events
        .iter()
        .filter(|e| matches!(&e.kind, EventKind::AttentionRaised { role, reason, .. } if role == "director" && reason == OUTWARD_ACTION))
        .count();
    assert_eq!(card_count, 1);
    assert_eq!(refused_count, 2);
    assert_eq!(attn_count, 1);
}

#[test]
fn agy_pre_tool_use_allows_outside_role_env_and_refuses_in_role_env() {
    let (s, mut role_env) = scratch();
    let cast = fixture(&s, "cast.toml", CAST_TOML);
    ok(
        &role_env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    role_env
        .vars
        .insert("BACKLOT_ROLE".into(), "director".into());

    let sig = s.files.join("signals.jsonl");
    let args = vec![
        "--runner".into(),
        "agy".into(),
        "--file".into(),
        sig.to_string_lossy().into_owned(),
    ];
    let push_payload = r#"{"conversationId":"person-conv","hookEventName":"PreToolUse","toolCall":{"name":"run_command","args":{"CommandLine":"git push origin HEAD"}}}"#;

    // Outside a role env (person's own agy in the same work area): always allow, record nothing.
    let empty_out = run_agy(&args, &Env::default(), &s.files, push_payload);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(empty_out.trim()).unwrap()["decision"],
        "allow"
    );
    let (events_before, _) = load_events(&studio_dir(&s.dir, "acme-shop")).unwrap();
    assert!(events_before.iter().all(|e| !matches!(
        e.kind,
        EventKind::RoleRefused { .. } | EventKind::AttentionRaised { .. }
    )));

    // Same payload inside a role env: denied and recorded.
    let role_out = run_agy(&args, &role_env, &s.files, push_payload);
    let parsed: serde_json::Value = serde_json::from_str(role_out.trim()).unwrap();
    assert_eq!(parsed["decision"], "deny");
    assert_eq!(
        parsed["reason"],
        "refused by backlot (unrecorded remote URL 'origin'): sent to the person as a card; wait for the result"
    );
    let (events_after, _) = load_events(&studio_dir(&s.dir, "acme-shop")).unwrap();
    assert_eq!(
        events_after
            .iter()
            .filter(|e| matches!(e.kind, EventKind::RoleRefused { .. }))
            .count(),
        1
    );
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
    let (code, done) = run_io_with_delay(
        &args,
        &env,
        &s.files,
        Duration::from_millis(20),
        Some(Duration::from_millis(120)),
        &mut Cursor::new(push_payload.as_bytes()),
        &mut out,
    );
    done.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(code, ExitCode::SUCCESS);
    let timed_out = String::from_utf8(out).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(timed_out.trim()).unwrap()["decision"],
        "allow"
    );
}

#[test]
fn agy_pre_tool_use_blocks_self_approval_backlot_env_and_mutations() {
    let (s, env_base) = scratch();
    let cast = fixture(&s, "cast.toml", CAST_TOML);
    ok(
        &env_base,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let sig = s.files.join("signals.jsonl");
    let args = vec![
        "--runner".into(),
        "agy".into(),
        "--file".into(),
        sig.to_string_lossy().into_owned(),
    ];
    let run_role = |role: &str, cmd: &str| -> String {
        let mut env = Env {
            vars: env_base.vars.clone(),
        };
        env.vars.insert("BACKLOT_ROLE".into(), role.into());
        let payload = serde_json::json!({
            "conversationId": "conv-h3",
            "hookEventName": "PreToolUse",
            "toolCall": { "name": "run_command", "args": { "CommandLine": cmd } }
        })
        .to_string();
        let out = run_agy(&args, &env, &s.files, &payload);
        serde_json::from_str::<serde_json::Value>(out.trim()).unwrap()["decision"]
            .as_str()
            .unwrap()
            .to_string()
    };

    for cmd in [
        "echo $BACKLOT_ROLE",
        "env -i backlot card apply c1",
        "env --ignore-environment backlot card apply c1",
        "backlot card apply c1",
        "backlot card cancel c1",
        "backlot --studio acme-shop card apply c1",
        "backlot perms revoke r1",
        "backlot approve --person actor-t1-c1 --decision allow",
    ] {
        assert_eq!(
            run_role("actor-t1-c1", cmd),
            "deny",
            "actor-t1-c1 must deny `{cmd}`"
        );
    }

    for role in ["writer", "hollywood"] {
        assert_eq!(
            run_role(
                role,
                "backlot approve --person actor-t1-c1 --decision allow"
            ),
            "deny",
            "{role} must deny `backlot approve`"
        );
    }

    for bad in [
        "backlot approve --as user --person actor-t1-c1 --decision allow",
        "backlot approve --person director --decision allow",
        "backlot card apply c1",
    ] {
        assert_eq!(
            run_role("director", bad),
            "deny",
            "director must deny `{bad}`"
        );
    }
    assert_eq!(
        run_role(
            "director",
            "backlot approve --person actor-t1-c1 --decision allow"
        ),
        "allow"
    );
    assert_eq!(run_role("actor-t1-c1", "backlot card list"), "allow");
}

/// The command-line layer refuses the role's ways around the seatbelt:
/// `hc` control, queue/socket scripts, detach tricks and server control.
/// Reads stay allowed, and the person's own agy outside a role env is
/// untouched.
#[test]
fn agy_pre_tool_use_refuses_role_escapes_and_keeps_reads() {
    let (s, env_base) = scratch();
    let cast = fixture(&s, "cast.toml", CAST_TOML);
    ok(
        &env_base,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let sig = s.files.join("signals.jsonl");
    let args = vec![
        "--runner".into(),
        "agy".into(),
        "--file".into(),
        sig.to_string_lossy().into_owned(),
    ];
    let mut env = Env {
        vars: env_base.vars.clone(),
    };
    env.vars.insert("BACKLOT_ROLE".into(), "producer".into());

    let decision = |env: &Env, cmd: &str| -> String {
        let payload = serde_json::json!({
            "conversationId": "conv-guard",
            "hookEventName": "PreToolUse",
            "toolCall": { "name": "run_command", "args": { "CommandLine": cmd } }
        })
        .to_string();
        let out = run_agy(&args, env, &s.files, &payload);
        serde_json::from_str::<serde_json::Value>(out.trim()).unwrap()["decision"]
            .as_str()
            .unwrap()
            .to_string()
    };

    for bad in [
        "hc run --bg 'backlot card apply c1'",
        "hc exec t1 -- backlot card apply c1",
        "hc send t1 hi",
        "hc keys t1 ctrl-c",
        "hc connect --session backlot",
        "python3 -c 'open(\"plugin.backlot.req\", \"w\").write(\"doc.set\")'",
        "nc -U /run/user/1000/herdr-core/backlot/backlot.sock",
        "systemd-run --user sh -c 'backlot card apply c1'",
        "tmux new -d sh -c 'backlot card apply c1'",
        "backlot server stop",
    ] {
        assert_eq!(decision(&env, bad), "deny", "must deny `{bad}`");
    }

    for good in [
        "hc ls",
        "hc get backlot",
        "hc file read a.txt",
        "git status",
    ] {
        assert_eq!(decision(&env, good), "allow", "must allow `{good}`");
    }

    // The person's own agy in the same work area never sees the guard.
    assert_eq!(
        decision(&Env::default(), "hc run --bg 'backlot card apply c1'"),
        "allow"
    );
}

//! PR 5 (`design-capture`) hook & `[design]` config tests: capture guards,
//! `agy` `PreInvocation` dedup, size capping, `signals.jsonl` isolation,
//! silent exit-0 failure paths, and layered `[design] hook_capture` config.

use std::fs;
use std::process::ExitCode;

use serde_json::{Value, json};

use crate::config::{DesignCfg, design_in};
use crate::design_capture::hook_capture_path;
use crate::design_capture_tests::{advance_to_waiting_user, role_env, run_hook_json, setup_studio};
use crate::hook::{LINE_CAP, run_io_env};
use crate::signals::signals_path;

#[test]
fn hook_capture_claude_agy_guards_caps_and_isolation() {
    let (scratch, env, dir) = setup_studio("required", false, "claude");
    let cap_path = hook_capture_path(&dir);
    let sig = signals_path(&dir, "producer");
    let prod_env = role_env(&env, "producer");

    // Not yet in WaitingUser -> nothing captured.
    run_hook_json(
        None,
        &sig,
        &prod_env,
        &scratch.files,
        json!({"session_id": "s-prod", "hook_event_name": "UserPromptSubmit", "prompt": "too early"}),
    );
    assert!(!cap_path.exists());

    advance_to_waiting_user(&scratch, &env);

    // Non-producer, `[backlot]` prefix, and blank prompt are ignored.
    run_hook_json(
        None,
        &sig,
        &role_env(&env, "director"),
        &scratch.files,
        json!({"session_id": "s-dir", "hook_event_name": "UserPromptSubmit", "prompt": "not producer"}),
    );
    run_hook_json(
        None,
        &sig,
        &prod_env,
        &scratch.files,
        json!({"session_id": "s-prod", "hook_event_name": "UserPromptSubmit", "prompt": "[backlot] notice"}),
    );
    run_hook_json(
        None,
        &sig,
        &prod_env,
        &scratch.files,
        json!({"session_id": "s-prod", "hook_event_name": "UserPromptSubmit", "prompt": "   \n "}),
    );
    assert!(!cap_path.exists());

    // `[design] hook_capture = false` in user config disables capture; state base overrides user config.
    let xdg_cfg = scratch.dir.join("xdg").join("backlot").join("config.toml");
    fs::create_dir_all(xdg_cfg.parent().unwrap()).unwrap();
    fs::write(&xdg_cfg, "[design]\nhook_capture = false\n").unwrap();
    run_hook_json(
        None,
        &sig,
        &prod_env,
        &scratch.files,
        json!({"session_id": "s-prod", "hook_event_name": "UserPromptSubmit", "prompt": "disabled user cfg"}),
    );
    assert!(!cap_path.exists());

    let core_cfg = scratch.dir.join("config.toml");
    fs::write(&core_cfg, "[design]\nhook_capture = false\n").unwrap();
    fs::write(&xdg_cfg, "[design]\nhook_capture = true\n").unwrap();
    run_hook_json(
        None,
        &sig,
        &prod_env,
        &scratch.files,
        json!({"session_id": "s-prod", "hook_event_name": "UserPromptSubmit", "prompt": "disabled core cfg"}),
    );
    assert!(!cap_path.exists());

    // State base enables capture cleanly over disabled user config; Claude verbatim + agy dedup & cap work.
    fs::write(&xdg_cfg, "[design]\nhook_capture = false\n").unwrap();
    fs::write(&core_cfg, "[design]\nhook_capture = true\n").unwrap();
    run_hook_json(
        None,
        &sig,
        &prod_env,
        &scratch.files,
        json!({"session_id": "s-prod", "hook_event_name": "UserPromptSubmit", "prompt": "Use OAuth 2.1 with PKCE"}),
    );
    let long_prompt = "X".repeat(LINE_CAP + 512);
    for inv_num in [0u64, 1u64] {
        run_hook_json(
            Some("agy"),
            &sig,
            &prod_env,
            &scratch.files,
            json!({
                "conversationId": "s-prod",
                "hookEventName": "PreInvocation",
                "invocationNum": inv_num,
                "lastUserInput": long_prompt,
            }),
        );
    }

    let captured = fs::read_to_string(&cap_path).expect(".hook-capture.jsonl should exist");
    let cap_lines: Vec<&str> = captured.lines().collect();
    assert_eq!(cap_lines.len(), 2, "{captured}");
    for raw in &cap_lines {
        assert!(raw.len() < LINE_CAP, "line len {}", raw.len());
    }
    let first: Value = serde_json::from_str(cap_lines[0]).unwrap();
    assert_eq!(first["prompt"], "Use OAuth 2.1 with PKCE");
    assert_eq!(first["version"], 1);
    assert_eq!(first["round"], 1);
    assert_eq!(first["session_id"], "s-prod");
    let second: Value = serde_json::from_str(cap_lines[1]).unwrap();
    assert!(second["prompt"].as_str().unwrap().starts_with("XXXX"));

    let signals_text = fs::read_to_string(&sig).unwrap();
    assert!(!signals_text.contains("Use OAuth 2.1 with PKCE"));
    assert!(!signals_text.contains("XXXX"));

    // Unwritable capture file still exits 0 silently.
    fs::remove_file(&cap_path).unwrap();
    fs::create_dir_all(&cap_path).unwrap();
    let mut out = Vec::new();
    let body =
        json!({"session_id": "s-prod", "hook_event_name": "UserPromptSubmit", "prompt": "safe"})
            .to_string();
    let code = run_io_env(
        &["--file".into(), sig.to_string_lossy().into_owned()],
        &prod_env,
        &scratch.files,
        std::time::Duration::from_millis(1500),
        &mut std::io::Cursor::new(body.as_bytes()),
        &mut out,
    );
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(out.is_empty());
}

#[test]
fn design_config_parses_bool_and_rejects_bad_types() {
    assert_eq!(design_in("").unwrap(), DesignCfg { hook_capture: true });
    assert_eq!(
        design_in("[design]\nhook_capture = false\n").unwrap(),
        DesignCfg {
            hook_capture: false
        }
    );
    assert_eq!(
        design_in("[design]\nhook_capture = true\n").unwrap(),
        DesignCfg { hook_capture: true }
    );
    assert!(
        design_in("design = true\n")
            .unwrap_err()
            .contains("not a table")
    );
    assert!(
        design_in("[design]\nhook_capture = \"no\"\n")
            .unwrap_err()
            .contains("hook_capture")
    );
}

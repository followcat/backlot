//! Pre-tool-hook integration for provably read-only shell commands.

use std::path::{Path, PathBuf};

fn field<'a>(v: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(|x| x.as_str())
}

fn shell_command(runner: &str, payload: &serde_json::Value) -> Option<String> {
    if runner == "agy" {
        let tc = payload.get("toolCall")?;
        if field(tc, "name") != Some("run_command") {
            return None;
        }
        let args = tc.get("args")?;
        return args
            .get("CommandLine")
            .or_else(|| args.get("commandLine"))
            .or_else(|| args.get("command"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
    }
    if runner == "cursor" && field(payload, "hook_event_name") == Some("beforeShellExecution") {
        return field(payload, "command").map(str::to_string);
    }
    let event = field(payload, "hook_event_name").or_else(|| field(payload, "hookEventName"));
    let decision_event = if runner == "devin" {
        // Devin's public hook reference auto-approves at PermissionRequest;
        // PreToolUse is earlier than the permission decision there.
        Some("PermissionRequest")
    } else {
        Some("PreToolUse")
    };
    if event != decision_event {
        return None;
    }
    payload
        .get("tool_input")
        .or_else(|| payload.get("toolInput"))
        .and_then(|input| field(input, "command"))
        .map(str::to_string)
}

fn allow_json(runner: &str) -> Option<String> {
    let value = match runner {
        "agy" => serde_json::json!({"decision":"allow"}),
        "devin" => serde_json::json!({"decision":"approve","reason":"provably read-only command"}),
        "cursor" => {
            serde_json::json!({"permission":"allow","user_message":"provably read-only command"})
        }
        "claude" | "cmd" => serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "allow",
                "permissionDecisionReason": "provably read-only command"
            }
        }),
        _ => return None,
    };
    serde_json::to_string(&value).ok()
}

pub(crate) fn auto_line(command: &str, cwd: &Path, runner: &str, ts_ms: u64) -> String {
    serde_json::to_string(&serde_json::json!({
        "ts_ms": ts_ms,
        "event": "PermitAutoAllow",
        "command": crate::hook::cut(command, 1800),
        "cwd": crate::hook::cut(&cwd.to_string_lossy(), 1800),
        "decision": "allow",
        "reason": "provably read-only command",
        "runner": runner,
    }))
    .unwrap_or_default()
        + "\n"
}

/// Assess one pre-tool payload. Guards classify first; only a plain
/// guard-allow proceeds to the read-only classifier.
pub(crate) fn decision(
    runner: &str,
    payload: &serde_json::Value,
    env: &crate::cli::Env,
    ts_ms: u64,
) -> Option<(String, String)> {
    let command = shell_command(runner, payload)?;
    let cwd = PathBuf::from(crate::hook::payload_cwd(payload)?.trim());
    let (studio_dir, _role) = crate::guard::studio_role_from_env(env)?;
    let info = crate::guard::load_studio_guard_info(env);
    let work_area = info.work_area.clone()?;
    let role_dir = info.role_dir.clone()?;
    let ctx = crate::guard_git::GitContext::from_info(&info, &info.policy, &cwd, env);
    if !matches!(
        crate::guard_shell::classify_command_line_with_ctx(&command, &ctx).verdict,
        crate::guard_rules::GuardVerdict::Allow
    ) {
        return None;
    }
    let repo = info.repo_root.unwrap_or_else(|| PathBuf::from(&studio_dir));
    let roots = crate::readonly_cmd::roots(&studio_dir, &repo, &role_dir, &work_area);
    let home = env.get("HOME").map(PathBuf::from);
    crate::readonly_cmd::check_command(&command, &cwd, &roots, home.as_deref()).ok()?;
    Some((
        allow_json(runner)?,
        auto_line(&command, &cwd, runner, ts_ms),
    ))
}

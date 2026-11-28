//! Devin: like codex the runner assigns the session id — `devin list
//! --format json` in the role's working directory finds it after
//! launch. No effort flag: the depth is part of the model id. A resume
//! ignores `--prompt-file`, so the prompt is not part of its argv.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use backlot_model::ActorIdentity;

use super::hook_files::{hooks_member_json, is_tracked, maybe_exclude_new, work_dir_of};
use super::splice::{splice_member, write600};
use super::{Launch, LaunchCtx, PlanError, Prep, PrepNote, Session};

pub const DEVIN_PREP_FAILED: &str = "devin prep failed";

/// Per-work-area devin hook configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct DevinPrep {
    /// `<cwd>/.devin/config.local.json`.
    pub config: PathBuf,
    /// `<studio>/roles/<role>/signals.jsonl`.
    pub signals: PathBuf,
    /// The role's read-only pre-allow rules — one `Exec(<cmd>)` entry
    /// under `permissions.allow`, in Devin's own rule syntax. Devin's
    /// rules are prefix matches (`Exec(ls)` permits `ls -a`), so there
    /// is no separate ` *` form. Devin's own `--permission-mode smart`
    /// stays the gate; these only spare the role a prompt for the
    /// forensics every role runs.
    pub allow: Vec<String>,
    /// When true, add `Write(*)` to `permissions.deny`.
    pub readonly: bool,
}

pub(crate) const HOOK_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "Stop",
    "SessionEnd",
];

/// The read-only pre-allow rules a role's `permissions.allow` gets —
/// one `Exec(<cmd>)` per shared read-only command ([`super::READ_ONLY`]).
/// Devin's rules are prefix-matched on the command line (`Exec(ls)`
/// permits `ls -a`, `Exec(grep)` permits `grep -n x`), so a single
/// entry per command is the whole rule; there is no ` *` form and no
/// `Bash(...)`. Nothing a role's job adds (an actor's commits, the
/// director's merge, a check's `run`) is pre-allowed here: Devin
/// takes no argv allow list and its smart mode governs the rest.
pub(crate) fn allow_rules() -> Vec<String> {
    super::native_read_only()
        .map(|cmd| format!("Exec({cmd})"))
        .collect()
}

/// Build or byte-preservingly splice `<cwd>/.devin/config.local.json`:
/// `hooks` and — when `allow` is non-empty or `readonly` sets
/// `permissions.deny` — `permissions`. Every other key keeps its bytes.
#[cfg(test)]
pub(crate) fn config_content(existing: Option<&[u8]>, signals: &Path, allow: &[String]) -> Vec<u8> {
    config_content_for(existing, signals, allow, false)
}

fn config_content_for(
    existing: Option<&[u8]>,
    signals: &Path,
    allow: &[String],
    readonly: bool,
) -> Vec<u8> {
    let text = existing.and_then(|b| std::str::from_utf8(b).ok());
    let hooks_json = hooks_member_json(text, HOOK_EVENTS, true, "devin", signals);
    let fresh = || format!("{{\"hooks\":{hooks_json}}}\n").into_bytes();
    let Some(raw) = text else {
        return with_permissions(fresh(), allow, readonly);
    };
    match splice_member(raw.trim(), "hooks", &hooks_json) {
        Some(spliced) => with_permissions(format!("{spliced}\n").into_bytes(), allow, readonly),
        None => with_permissions(fresh(), allow, readonly),
    }
}

/// Splice `permissions.allow` (when non-empty) and `Write(*)` into
/// `permissions.deny` (when `readonly`) while preserving any other
/// `permissions` entries. A file that no longer parses stays unchanged.
fn with_permissions(bytes: Vec<u8>, allow: &[String], readonly: bool) -> Vec<u8> {
    if allow.is_empty() && !readonly {
        return bytes;
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return bytes;
    };
    let mut permissions: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str::<serde_json::Value>(text.trim())
            .ok()
            .and_then(|v| v.get("permissions").and_then(|p| p.as_object()).cloned())
            .unwrap_or_default();
    if !allow.is_empty() {
        permissions.insert("allow".into(), serde_json::json!(allow));
    }
    if readonly {
        let mut deny = permissions
            .get("deny")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();
        if !deny.iter().any(|v| v.as_str() == Some("Write(*)")) {
            deny.push(serde_json::json!("Write(*)"));
        }
        permissions.insert("deny".into(), serde_json::Value::Array(deny));
    }
    let value = serde_json::to_string(&serde_json::Value::Object(permissions)).unwrap_or_default();
    match splice_member(text.trim(), "permissions", &value) {
        Some(spliced) => format!("{spliced}\n").into_bytes(),
        None => bytes,
    }
}

/// Write `.devin/config.local.json` and exclude it if new; skip without
/// dirtying when the repo already tracks it.
pub fn prepare(p: &DevinPrep) -> Result<Option<PrepNote>, String> {
    const REL: &str = ".devin/config.local.json";
    let cwd = work_dir_of(&p.config);
    if is_tracked(cwd, REL) {
        return Ok(Some(PrepNote::HooksUnavailable(REL)));
    }
    let was_new = !p.config.exists();
    let dir = p.config.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let existing = std::fs::read(&p.config).ok();
    let bytes = config_content_for(existing.as_deref(), &p.signals, &p.allow, p.readonly);
    write600(&p.config, &bytes).map_err(|e| format!("{}: {e}", p.config.display()))?;
    maybe_exclude_new(cwd, was_new, REL)?;
    Ok(None)
}

/// The models `devin --model` is offered: `adaptive`, the SWE-2.1
/// and SWE-2 families (the runner's own) and one `-high` variant per
/// third-party family — `devin models` prints over a thousand
/// ids on the measured account, the depth baked into each one, so
/// the catalog names the headliners and free text covers the rest.
pub const MODELS: &[&str] = &[
    "adaptive",
    "swe-2-1-max",
    "swe-2-1-xhigh",
    "swe-2-1-high",
    "swe-2-1-medium",
    "swe-2-1-max-lightning",
    "swe-2-1-high-lightning",
    "swe-2-max",
    "swe-2-high",
    "swe-2-medium",
    "swe-2-max-lightning",
    "swe-2-high-lightning",
    "swe-2-medium-lightning",
    "swe-2-flash-max",
    "swe-2-flash-high",
    "swe-2-flash-medium",
    "claude-opus-5-5-high",
    "claude-sonnet-5-5-high",
    "claude-opus-5-high",
    "claude-opus-4-8-high",
    "claude-opus-4-7-high",
    "claude-fable-5-2-high",
    "claude-fable-5-1-high",
    "claude-5-fable-high",
    "claude-sonnet-5-high",
    "claude-haiku-5-5-high",
    "gpt-6-2-astra-high",
    "gpt-6-astra-high",
    "gpt-6-sol-high",
    "gpt-6-luna-high",
    "gpt-5-6-sol-high",
    "gpt-5-6-terra-high",
    "gemini-4-carbon-high",
    "gemini-4-neon-high",
    "gemini-4-hydrogen-high",
    "gemini-4-argon-high",
    "gemini-3-8-flash-high",
    "gemini-3-7-flash-high",
    "kimi-k3-1-pro-high",
    "glm-5-3-high",
];

pub fn plan(identity: &ActorIdentity, ctx: &LaunchCtx) -> Result<Launch, PlanError> {
    let readonly = match identity.permission_profile.as_str() {
        "standard" | "worktree" => false,
        "readonly" => true,
        other => return Err(PlanError::profile(other)),
    };
    let mut argv = vec!["devin".to_string()];
    if let Some(id) = ctx.resume {
        argv.extend(["-r".into(), id.to_string()]);
    }
    argv.extend([
        "--model".into(),
        identity.model.clone(),
        "--permission-mode".into(),
        "smart".into(),
        "--respect-workspace-trust".into(),
        "false".into(),
    ]);
    if ctx.resume.is_none()
        && let Some(f) = ctx.prompt
    {
        argv.extend(["--prompt-file".into(), f.to_string()]);
    }
    Ok(Launch {
        argv,
        env: BTreeMap::new(),
        session: Session::PostLaunch,
        hooks: None,
        prep: Some(Prep::Devin(DevinPrep {
            config: Path::new(ctx.cwd).join(".devin").join("config.local.json"),
            signals: Path::new(ctx.studio_dir)
                .join("roles")
                .join(ctx.role)
                .join("signals.jsonl"),
            // The shared read-only set for a studio role; Hollywood is
            // the overseer and takes none (D17a keeps it to backlot's
            // own commands, which it is not pre-allowed here at all).
            allow: if ctx.role == "hollywood" {
                Vec::new()
            } else {
                allow_rules()
            },
            readonly,
        })),
    })
}

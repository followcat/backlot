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

/// Build or byte-preservingly splice `<cwd>/.devin/config.local.json`.
pub(crate) fn config_content(existing: Option<&[u8]>, signals: &Path) -> Vec<u8> {
    let text = existing.and_then(|b| std::str::from_utf8(b).ok());
    let hooks_json = hooks_member_json(text, HOOK_EVENTS, true, "devin", signals);
    let fresh = || format!("{{\"hooks\":{hooks_json}}}\n").into_bytes();
    let Some(raw) = text else {
        return fresh();
    };
    match splice_member(raw.trim(), "hooks", &hooks_json) {
        Some(spliced) => format!("{spliced}\n").into_bytes(),
        None => fresh(),
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
    let bytes = config_content(existing.as_deref(), &p.signals);
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
    match identity.permission_profile.as_str() {
        "standard" | "worktree" => {}
        other => return Err(PlanError::profile(other)),
    }
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
        })),
    })
}

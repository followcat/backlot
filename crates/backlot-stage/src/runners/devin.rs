//! Devin: like codex the runner assigns the session id — `devin list
//! --format json` in the role's working directory finds it after
//! launch. No effort flag: the depth is part of the model id. A resume
//! ignores `--prompt-file`, so the prompt is not part of its argv.

use std::collections::BTreeMap;

use backlot_model::ActorIdentity;

use super::{Launch, LaunchCtx, PlanError, Session};

/// The models `devin --model` is offered: `adaptive`, the SWE-2
/// families (the runner's own) and one `-high` variant per
/// third-party family — `devin models` prints over a thousand
/// ids on the measured account, the depth baked into each one, so
/// the catalog names the headliners and free text covers the rest.
pub const MODELS: &[&str] = &[
    "adaptive",
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
        prep: None,
    })
}

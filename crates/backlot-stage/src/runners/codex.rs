//! Codex: the runner assigns the session id — the stage finds it in
//! `~/.codex/sessions/**/rollout-*.jsonl` after launch and records
//! `role.session`. The `projects` inline table marks this launch's
//! working directory trusted for the process only, keeping the trust
//! answer out of the user's config; the dotted form does not work.

use std::collections::BTreeMap;

use backlot_model::ActorIdentity;

use super::{Launch, LaunchCtx, PlanError, Session};

/// The models `codex -m` is offered: the slugs the CLI's own
/// `models_cache.json` — what the `/model` picker shows — carried
/// on the measured account, minus `codex-auto-review`, which
/// serves the review feature rather than a session. A slug the
/// list does not name still goes in as free text.
pub const MODELS: &[&str] = &[
    "gpt-6.1-sol",
    "gpt-6-astra",
    "gpt-6.1-terra",
    "gpt-6-sol",
    "gpt-reserve",
    "gpt-6-luna",
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
];

pub fn plan(identity: &ActorIdentity, ctx: &LaunchCtx) -> Result<Launch, PlanError> {
    match identity.permission_profile.as_str() {
        "standard" | "worktree" => {}
        other => return Err(PlanError::profile(other)),
    }
    let mut argv = vec!["codex".to_string()];
    if let Some(id) = ctx.resume {
        argv.extend(["resume".into(), id.to_string()]);
    }
    argv.extend([
        "-m".into(),
        identity.model.clone(),
        "-c".into(),
        format!(
            "model_reasoning_effort={}",
            identity.effort.as_deref().unwrap_or_default()
        ),
        "-c".into(),
        format!("projects={{\"{}\"={{trust_level=\"trusted\"}}}}", ctx.cwd),
        "-s".into(),
        "workspace-write".into(),
        "-a".into(),
        "on-request".into(),
    ]);
    if let Some(p) = ctx.prompt {
        argv.push(p.into());
    }
    Ok(Launch {
        argv,
        env: BTreeMap::new(),
        session: Session::PostLaunch,
        hooks: None,
        prep: None,
    })
}

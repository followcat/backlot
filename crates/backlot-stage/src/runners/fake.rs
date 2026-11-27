//! The `fake` runner: a shipped script that announces the role and
//! stays alive. Its session id derives from the terminal name, so a
//! recorded id survives a role close. `model = "trust"` puts it in
//! the real-core test's trust-dialog mode through `FAKE_MODEL`.
//! S9f: it plays the same hooks Claude does — the same settings
//! file and `--settings` argv, the hook command handed to it as
//! `BACKLOT_HOOK_CMD` for its `hook:` inputs, and the session id
//! it emits as `FAKE_SESSION`.

use backlot_model::ActorIdentity;

use super::{Launch, LaunchCtx, PlanError, Session, probe};

/// The script's model-driven modes: `trust` plays the folder-trust
/// dialog, anything else a plain run — `m` is the draft's.
pub const MODELS: &[&str] = &["m", "trust"];

pub fn plan(identity: &ActorIdentity, ctx: &LaunchCtx) -> Result<Launch, PlanError> {
    let _ = ctx.resume; // the script stays the same; the id re-derives.
    let mut env = std::collections::BTreeMap::new();
    env.insert("FAKE_MODEL".into(), identity.model.clone());
    env.insert("FAKE_SESSION".into(), format!("fake-{}", ctx.terminal));
    if let Some(prompt) = ctx.prompt {
        env.insert("FAKE_PROMPT".into(), prompt.to_owned());
    }
    let hooks = super::claude::hook_setup_with_deny(ctx, identity.permission_profile == "readonly");
    env.insert("BACKLOT_HOOK_CMD".into(), hooks.command.clone());
    let override_path = ctx
        .stage_env
        .get(probe::FAKE_RUNNER_ENV)
        .map(String::as_str);
    let script = probe::fake_runner_path(probe::fake_runner_override(override_path).as_deref());
    Ok(Launch {
        argv: vec![
            script.to_string_lossy().into_owned(),
            "--settings".into(),
            hooks.settings.to_string_lossy().into_owned(),
        ],
        env,
        session: Session::Chosen(format!("fake-{}", ctx.terminal)),
        hooks: Some(hooks),
        prep: None,
    })
}

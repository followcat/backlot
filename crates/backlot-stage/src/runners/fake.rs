//! The `fake` runner: a shipped script that announces the role and
//! stays alive. Its session id derives from the terminal name, so a
//! recorded id survives a role close. `model = "trust"` puts it in
//! the real-core test's trust-dialog mode through `FAKE_MODEL`.
//! S9f: it plays the same hooks Claude does — the same settings
//! file and `--settings` argv, the hook command handed to it as
//! `BACKLOT_HOOK_CMD` for its `hook:` inputs, and the session id
//! it emits as `FAKE_SESSION`.

use backlot_model::ActorIdentity;

use super::{Launch, LaunchCtx, PlanError, Session, tools_dir};

pub fn plan(identity: &ActorIdentity, ctx: &LaunchCtx) -> Result<Launch, PlanError> {
    let _ = ctx.resume; // the script stays the same; the id re-derives.
    let mut env = std::collections::BTreeMap::new();
    env.insert("FAKE_MODEL".into(), identity.model.clone());
    env.insert("FAKE_SESSION".into(), format!("fake-{}", ctx.terminal));
    let hooks = super::claude::hook_setup(ctx);
    env.insert("BACKLOT_HOOK_CMD".into(), hooks.command.clone());
    Ok(Launch {
        argv: vec![
            tools_dir()
                .join("fake-runner.sh")
                .to_string_lossy()
                .into_owned(),
            "--settings".into(),
            hooks.settings.to_string_lossy().into_owned(),
        ],
        env,
        session: Session::Chosen(format!("fake-{}", ctx.terminal)),
        hooks: Some(hooks),
        prep: None,
    })
}

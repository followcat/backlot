//! The `fake` runner: a shipped script that announces the role and
//! stays alive. Its session id derives from the terminal name, so a
//! recorded id survives a role close. `model = "trust"` puts it in
//! the real-core test's trust-dialog mode through `FAKE_MODEL`.

use backlot_model::ActorIdentity;

use super::{Launch, LaunchCtx, PlanError, Session, tools_dir};

pub fn plan(identity: &ActorIdentity, ctx: &LaunchCtx) -> Result<Launch, PlanError> {
    let _ = ctx.resume; // the script stays the same; the id re-derives.
    let mut env = std::collections::BTreeMap::new();
    env.insert("FAKE_MODEL".into(), identity.model.clone());
    Ok(Launch {
        argv: vec![
            tools_dir()
                .join("fake-runner.sh")
                .to_string_lossy()
                .into_owned(),
        ],
        env,
        session: Session::Chosen(format!("fake-{}", ctx.terminal)),
        hooks: None,
    })
}

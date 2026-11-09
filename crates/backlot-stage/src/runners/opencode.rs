//! OpenCode: `-s` chooses the session id at launch — it must start
//! with `ses` — so `role.started` carries it. The interactive command
//! has no model flag: the model goes through `OPENCODE_CONFIG_CONTENT`
//! with `--standalone`, which the background service needs to see it.
//! Effort is not applied either — `#variant` exists only on
//! `opencode run` and the adapter does not guess at it.

use std::collections::BTreeMap;

use backlot_model::ActorIdentity;

use super::{Launch, LaunchCtx, PlanError, Session, random_16};

/// `ses_` plus 16 random bytes as hex.
fn new_session_id() -> String {
    let mut id = String::from("ses_");
    for b in random_16() {
        id.push_str(&format!("{b:02x}"));
    }
    id
}

pub fn plan(identity: &ActorIdentity, ctx: &LaunchCtx) -> Result<Launch, PlanError> {
    match identity.permission_profile.as_str() {
        // Neither profile adds flags; only an unknown one refuses.
        "standard" | "worktree" => {}
        other => return Err(PlanError::profile(other)),
    }
    let session = ctx
        .resume
        .map(str::to_string)
        .unwrap_or_else(new_session_id);
    let mut argv = vec![
        "opencode".to_string(),
        "--standalone".into(),
        "-s".into(),
        session.clone(),
    ];
    if let Some(p) = ctx.prompt {
        argv.extend(["--prompt".into(), p.to_string()]);
    }
    let mut env = BTreeMap::new();
    env.insert(
        "OPENCODE_CONFIG_CONTENT".into(),
        serde_json::json!({"model": identity.model}).to_string(),
    );
    Ok(Launch {
        argv,
        env,
        session: Session::Chosen(session),
    })
}

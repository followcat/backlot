//! Devin: like codex the runner assigns the session id — `devin list
//! --format json` in the role's working directory finds it after
//! launch. No effort flag: the depth is part of the model id. A resume
//! ignores `--prompt-file`, so the prompt is not part of its argv.

use std::collections::BTreeMap;

use backlot_model::ActorIdentity;

use super::{Launch, LaunchCtx, PlanError, Session};

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
    })
}

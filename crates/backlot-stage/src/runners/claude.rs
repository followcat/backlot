//! Claude Code: the id is a v4 UUID the adapter chooses at launch, so
//! `role.started` carries it. A Claude Code started under another
//! Claude Code inherits markers — `CLAUDE_CODE_CHILD_SESSION` turns
//! off transcript saving and nothing could be read back — so argv is
//! prefixed with `env -u <name>` for each one the stage's own env has.
//! `HC_*` is hc's per-terminal set and stays.

use std::collections::BTreeMap;

use backlot_model::ActorIdentity;

use super::{Launch, LaunchCtx, PlanError, Session, uuid_v4};

/// The marker names a parent Claude Code leaves in its environment.
fn is_marker(name: &str) -> bool {
    matches!(
        name,
        "CLAUDECODE" | "CLAUDE_EFFORT" | "CLAUDE_PID" | "AI_AGENT"
    ) || name.starts_with("CLAUDE_CODE_")
}

/// `--permission-mode`'s value for the profile.
fn permission_mode(profile: &str) -> Result<&'static str, PlanError> {
    match profile {
        "standard" => Ok("manual"),
        "worktree" => Ok("acceptEdits"),
        other => Err(PlanError::profile(other)),
    }
}

pub fn plan(identity: &ActorIdentity, ctx: &LaunchCtx) -> Result<Launch, PlanError> {
    let mode = permission_mode(&identity.permission_profile)?;
    let mut argv: Vec<String> = Vec::new();
    let markers: Vec<&String> = ctx.stage_env.keys().filter(|k| is_marker(k)).collect();
    if !markers.is_empty() {
        argv.push("env".into());
        for name in markers {
            argv.push("-u".into());
            argv.push(name.clone());
        }
    }
    argv.push("claude".into());
    let session = match ctx.resume {
        Some(id) => {
            argv.extend(["-r".into(), id.to_string()]);
            id.to_string()
        }
        None => uuid_v4(),
    };
    argv.extend([
        "--model".into(),
        identity.model.clone(),
        "--effort".into(),
        identity.effort.clone(),
        "--permission-mode".into(),
        mode.into(),
    ]);
    if ctx.resume.is_none() {
        argv.extend(["--session-id".into(), session.clone()]);
    }
    if let Some(p) = ctx.prompt {
        argv.push(p.into());
    }
    Ok(Launch {
        argv,
        env: BTreeMap::new(),
        session: Session::Chosen(session),
    })
}

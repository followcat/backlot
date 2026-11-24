//! Recasting a fixed role (`role.recast`): the reduced-state rule the
//! CLI, a Hollywood card and the append boundary share. The cast file's
//! identity rules hold again — the runner has an adapter, the effort
//! fits the runner, a cline model names its provider, the permission
//! profile is one of the two — and the studio's `[limits]` must still
//! seat its fixed roles after the change.

use crate::event::ActorIdentity;
use crate::refusal::Refusal;
use crate::state::StudioState;

/// Every runner name with an adapter — the same set the stage's
/// `runners::supported` answers.
pub const SUPPORTED_RUNNERS: &[&str] = &[
    "fake", "claude", "codex", "devin", "opencode", "cursor", "cmd", "cline", "agy",
];

/// The three fixed roles this event may name.
pub const FIXED_ROLES: &[&str] = &["producer", "director", "writer"];

/// Feedback note when recasting a fixed role that has not started yet.
pub fn unstarted_note(state: &StudioState, role: &str) -> Option<&'static str> {
    if state.roles.contains_key(role) {
        return None;
    }
    Some(match role {
        "writer" => "writer starts when the director asks for a plan",
        "producer" => "producer starts with the studio",
        "director" => "director starts with the studio",
        _ => return None,
    })
}

/// Whether `runner` has an adapter.
pub fn supported_runner(runner: &str) -> bool {
    SUPPORTED_RUNNERS.contains(&runner)
}

/// Two identities equal on everything the user sets — `runner_version`
/// is the stage's own reading of the binary, not a choice, so it never
/// makes a recast "different".
pub fn same_identity(a: &ActorIdentity, b: &ActorIdentity) -> bool {
    a.runner == b.runner
        && a.model == b.model
        && a.effort == b.effort
        && a.permission_profile == b.permission_profile
}

/// `<runner>/<model>[/<effort>]` — the one-line form the card text and
/// `card show` use.
pub fn identity_text(actor: &ActorIdentity) -> String {
    match actor.effort.as_deref().filter(|e| !e.is_empty()) {
        Some(effort) => format!("{}/{}/{effort}", actor.runner, actor.model),
        None => format!("{}/{}", actor.runner, actor.model),
    }
}

/// The cast file's effort rule, as `studio new` applies it — the
/// runner's caps decide.
fn check_effort(actor: &ActorIdentity) -> Result<(), Refusal> {
    use crate::caps::{self, EffortProblem};
    match caps::check_effort(&actor.runner, actor.effort.as_deref()) {
        Ok(()) => Ok(()),
        Err(EffortProblem::Missing) => Err(Refusal::EffortRequired {
            runner: actor.runner.clone(),
        }),
        Err(EffortProblem::NotAllowed) => Err(Refusal::EffortNotAllowed {
            runner: actor.runner.clone(),
        }),
    }
}

/// A cline model is a `cline-pass/<model>` id; any other names no
/// provider the adapter knows.
pub fn check_model(actor: &ActorIdentity) -> Result<(), Refusal> {
    if actor.runner != "cline" {
        return Ok(());
    }
    let names_provider = actor
        .model
        .strip_prefix("cline-pass/")
        .is_some_and(|rest| !rest.is_empty());
    if names_provider {
        Ok(())
    } else {
        Err(Refusal::ModelNotSupported {
            runner: actor.runner.clone(),
            model: actor.model.clone(),
        })
    }
}

/// After the recast, every capped runner still seats the fixed roles.
/// The producer and the director stay live for the whole run; a cap at
/// their count while the writer or a candidate shares the runner
/// leaves that person nowhere to sit.
fn check_limits(state: &StudioState, role: &str, actor: &ActorIdentity) -> Result<(), Refusal> {
    let Some(cfg) = &state.studio else {
        return Ok(());
    };
    let identity = |r: &str| -> Option<&ActorIdentity> {
        if r == role {
            Some(actor)
        } else {
            state.fixed_identity(r)
        }
    };
    let (Some(producer), Some(director), Some(writer)) = (
        identity("producer"),
        identity("director"),
        identity("writer"),
    ) else {
        return Ok(());
    };
    for (runner, cap) in &cfg.limits {
        let fixed = [producer, director]
            .into_iter()
            .filter(|a| a.runner == *runner)
            .count() as u32;
        if *cap < fixed {
            return Err(Refusal::LimitsUnseated {
                runner: runner.clone(),
                cap: *cap,
            });
        }
        let someone_waits =
            writer.runner == *runner || cfg.candidates.iter().any(|c| c.runner == *runner);
        if *cap == fixed && someone_waits {
            return Err(Refusal::LimitsUnseated {
                runner: runner.clone(),
                cap: *cap,
            });
        }
    }
    Ok(())
}

/// The whole recast rule, over the state it lands on. Runs at the
/// append boundary and from both the CLI and a card's proposal, so one
/// refusal reads the same wherever it comes from.
pub fn check(state: &StudioState, role: &str, actor: &ActorIdentity) -> Result<(), Refusal> {
    if !FIXED_ROLES.contains(&role) {
        return Err(Refusal::NotFixedRole(role.to_string()));
    }
    if state.studio.is_none() {
        return Err(Refusal::NoStudio);
    }
    if state.finished.is_some() {
        return Err(Refusal::AlreadyFinished);
    }
    match state.fixed_identity(role) {
        Some(current) if same_identity(current, actor) => {
            return Err(Refusal::RecastUnchanged(role.to_string()));
        }
        None => return Err(Refusal::NoRole(role.to_string())),
        Some(_) => {}
    }
    if !supported_runner(&actor.runner) {
        return Err(Refusal::RunnerNotSupported(actor.runner.clone()));
    }
    if !matches!(actor.permission_profile.as_str(), "standard" | "worktree") {
        return Err(Refusal::ProfileNotSupported(
            actor.permission_profile.clone(),
        ));
    }
    check_effort(actor)?;
    check_model(actor)?;
    check_limits(state, role, actor)
}

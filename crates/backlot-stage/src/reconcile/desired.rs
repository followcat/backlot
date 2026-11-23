//! The wanted set: which roles the ledger wants live right now,
//! each as a `Start` — fresh or resuming a held person's recorded
//! session. Pure over the reduced state; `reconcile` diffs it
//! against the live view. `wanted::wanted_roles` owns who is wanted
//! and the slot order — the runner limit reads the same list.

use backlot_model::event::{REOPENED, RESTARTED};
use backlot_model::state::StudioState;

use super::{Resume, RoleKind, Start, terminal_name};

/// The `RoleKind` a recorded role key resolves to — the same shape
/// `desired` builds for fresh starts.
fn kind_of(role_key: &str) -> RoleKind {
    match role_key {
        "producer" => RoleKind::Producer,
        "director" => RoleKind::Director,
        "writer" => RoleKind::Writer,
        _ => RoleKind::Actor,
    }
}

/// The resume a recorded role's start carries: a held person comes
/// back into the session it closed with (`reopened`); a `role.restart`
/// resumes the recorded session (`restarted`) unless it asked for a
/// fresh one. `None` is a fresh start.
fn resume_of(r: &backlot_model::state::RoleState) -> Option<Resume> {
    if let Some(req) = &r.restart {
        if req.fresh {
            return None;
        }
        return r.session_id.clone().map(|session| Resume {
            session,
            reason: RESTARTED,
        });
    }
    if r.held {
        return r.session_id.clone().map(|session| Resume {
            session,
            reason: REOPENED,
        });
    }
    None
}

/// Roles the ledger wants live right now, in slot order — producer,
/// director, writer, then actors by task priority and task seq. A
/// held person comes back with its recorded session (R19); a role a
/// `role.restart` named comes back resumed (`restarted`) or fresh,
/// as it asked. A wanted role with no actor has nothing to launch.
pub fn desired(studio: &str, state: &StudioState) -> Vec<Start> {
    let Some(cfg) = &state.studio else {
        return Vec::new();
    };
    backlot_model::wanted::wanted_roles(state)
        .into_iter()
        .filter_map(|w| {
            let terminal = terminal_name(studio, &w.role)?;
            let r = state.roles.get(&w.role);
            let resume = r.and_then(resume_of);
            // A wanted key with no actor in the cast path launches
            // only when it recorded one.
            let kind = kind_of(&w.role);
            Some(Start {
                role: w.role,
                kind,
                task: w.task,
                terminal,
                actor: w.actor,
                repo: cfg.repo.clone(),
                members: cfg.members.clone(),
                no_git: cfg.no_git,
                resume,
            })
        })
        .collect()
}

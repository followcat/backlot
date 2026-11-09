//! The wanted set: which roles the ledger wants live right now,
//! each as a `Start` — fresh or resuming a held person's recorded
//! session. Pure over the reduced state; `reconcile` diffs it
//! against the live view.

use backlot_model::event::REOPENED;
use backlot_model::state::{StudioState, actor_task};

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

/// The writer is wanted from a `purpose: "plan"` message until the
/// next `purpose: "dispatch"` one.
fn writer_wanted(state: &StudioState) -> bool {
    state
        .messages
        .iter()
        .rev()
        .find(|m| m.purpose.is_some())
        .is_some_and(|m| m.purpose.as_deref() == Some("plan"))
}

/// Roles the ledger wants live right now. A finished studio wants
/// nobody on its own — the held pass below still re-launches a
/// person an explicit `role.reopen` held.
pub fn desired(studio: &str, state: &StudioState) -> Vec<Start> {
    let Some(cfg) = &state.studio else {
        return Vec::new();
    };
    let mut out: Vec<Start> = Vec::new();
    if state.finished.is_none() {
        let fixed = [
            ("producer", RoleKind::Producer, &cfg.roles.producer),
            ("director", RoleKind::Director, &cfg.roles.director),
        ];
        out.extend(fixed.into_iter().map(|(role, kind, actor)| Start {
            role: role.into(),
            kind,
            task: None,
            terminal: format!("bl-{studio}-{role}"),
            actor: actor.clone(),
            repo: cfg.repo.clone(),
            resume: None,
        }));
        if writer_wanted(state) {
            out.push(Start {
                role: "writer".into(),
                kind: RoleKind::Writer,
                task: None,
                terminal: format!("bl-{studio}-writer"),
                actor: cfg.roles.writer.clone(),
                repo: cfg.repo.clone(),
                resume: None,
            });
        }
        for t in state.tasks.values() {
            // Cast and not closed: running, delivered or awaiting rework
            // all keep their person until the task closes.
            if let Some(cast) = &t.cast
                && !t.status.is_terminal()
            {
                out.push(Start {
                    role: format!("actor-{}-c{}", t.id, t.casts.len()),
                    kind: RoleKind::Actor,
                    task: Some(t.id.clone()),
                    terminal: format!("bl-{studio}-{}", t.id),
                    actor: cast.actor.clone(),
                    repo: cfg.repo.clone(),
                    resume: None,
                });
            }
        }
    }
    // A held person joins the desired set with its recorded session
    // (R19). One already listed stays one entry, resume set either
    // way; a held role with no actor has nothing to launch.
    for want in out.iter_mut() {
        if let Some(r) = state.roles.get(&want.role)
            && r.held
        {
            want.resume = r.session_id.clone().map(|session| Resume {
                session,
                reason: REOPENED,
            });
        }
    }
    for (key, r) in &state.roles {
        if !r.held || out.iter().any(|w| w.role == *key) {
            continue;
        }
        let (Some(actor), Some(terminal), Some(session)) =
            (&r.actor, terminal_name(studio, key), &r.session_id)
        else {
            continue;
        };
        out.push(Start {
            role: key.clone(),
            kind: kind_of(key),
            task: actor_task(key).map(String::from),
            terminal,
            actor: actor.clone(),
            repo: cfg.repo.clone(),
            resume: Some(Resume {
                session: session.clone(),
                reason: REOPENED,
            }),
        });
    }
    out
}

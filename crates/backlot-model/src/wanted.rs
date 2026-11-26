//! The wanted role set, shared by the stage's `desired` and the
//! runner-limit pass (S9e): which roles the ledger wants live, in
//! slot order — producer, director, writer, then actors by task
//! priority (high first) and then task seq — so a runner limit hands
//! its slots to the same roles every caller sees.

use crate::event::ActorIdentity;
use crate::state::{StudioState, actor_task};

/// One wanted role: its key, the task an actor works on, and the
/// identity it launches under.
#[derive(Debug, Clone, PartialEq)]
pub struct Wanted {
    pub role: String,
    pub task: Option<String>,
    pub actor: ActorIdentity,
}

/// Fixed-role rank: producer, director, writer before any actor.
fn rank(role: &str) -> u8 {
    match role {
        "producer" => 0,
        "director" => 1,
        "writer" => 2,
        _ => 3,
    }
}

/// An actor's slot order: task priority high first, then the task's
/// creation seq — the task sequence, not the role key.
fn task_key(state: &StudioState, task: Option<&str>) -> (std::cmp::Reverse<u8>, u64) {
    let t = task.and_then(|id| state.tasks.get(id));
    (
        std::cmp::Reverse(t.map(|t| t.priority as u8).unwrap_or(0)),
        t.map(|t| t.created_seq).unwrap_or(u64::MAX),
    )
}

/// The writer is wanted from a `purpose: "plan"` message until the
/// next `purpose: "dispatch"` one. Other purposes — a `nudge`, the
/// producer's `owed` note — carry no command and change nothing.
fn writer_wanted(state: &StudioState) -> bool {
    if crate::design_steps::design_gate_active(state) {
        return crate::design_steps::writer_wanted_for_design(state);
    }
    state
        .messages
        .iter()
        .rev()
        .find(|m| matches!(m.purpose.as_deref(), Some("plan") | Some("dispatch")))
        .is_some_and(|m| m.purpose.as_deref() == Some("plan"))
}

/// A plain `tell` to `role` that still waits on it. The newest
/// purpose-free, source-free message addressed to the role keeps it
/// wanted while undelivered, and after delivery until the role writes
/// to the ledger again — it answered, or its turn's step-boundary
/// `progress` (the role prompts ask for one at each boundary). Keying
/// the release on the delivery time, never the queue time, keeps a
/// write the role made before the message reached it from closing the
/// role first.
fn tell_holds(state: &StudioState, role: &str) -> bool {
    let Some(m) = state
        .messages
        .iter()
        .rev()
        .find(|m| m.to == role && m.purpose.is_none() && m.source_seq.is_none() && !m.dropped)
    else {
        return false;
    };
    if !m.delivered {
        return true;
    }
    let delivered_ms = m.delivered_ms.unwrap_or(m.ts_ms);
    !state
        .last_by_role
        .get(role)
        .is_some_and(|last| last.ts_ms > delivered_ms)
}

/// The roles the ledger wants live, in slot order. A finished
/// studio wants nobody on its own — the held pass still re-launches
/// a person an explicit `role.reopen` held, and the restart pass a
/// person a `role.restart` named.
pub fn wanted_roles(state: &StudioState) -> Vec<Wanted> {
    let Some(cfg) = &state.studio else {
        return Vec::new();
    };
    let mut out: Vec<Wanted> = Vec::new();
    if state.finished.is_none() {
        // A fixed role's newest `role.recast` is the identity it is
        // wanted under; the cast file's identity is the starting one.
        let fixed = |role: &str, fallback: &ActorIdentity| {
            state
                .fixed_identity(role)
                .cloned()
                .unwrap_or_else(|| fallback.clone())
        };
        out.push(Wanted {
            role: "producer".into(),
            task: None,
            actor: fixed("producer", &cfg.roles.producer),
        });
        out.push(Wanted {
            role: "director".into(),
            task: None,
            actor: fixed("director", &cfg.roles.director),
        });
        if writer_wanted(state) {
            out.push(Wanted {
                role: "writer".into(),
                task: None,
                actor: fixed("writer", &cfg.roles.writer),
            });
        }
        for t in state.tasks.values() {
            // Cast and not closed: running, delivered or awaiting
            // rework all keep their person until the task closes.
            if let Some(cast) = &t.cast
                && !t.status.is_terminal()
            {
                out.push(Wanted {
                    role: format!("actor-{}-c{}", t.id, t.casts.len()),
                    task: Some(t.id.clone()),
                    actor: cast.actor.clone(),
                });
            }
        }
    }
    // A plain `tell` wakes a closed fixed role the same way a plan
    // request does: while a message still waits on the writer or the
    // director, the role is wanted so the stage starts it again and
    // delivers the message. An actor is never woken this way — its
    // task is done or abandoned, and the tell is refused at append
    // time (see `validate`). A finished studio wakes nobody.
    if state.finished.is_none() {
        for (role, fallback) in [
            ("writer", &cfg.roles.writer),
            ("director", &cfg.roles.director),
        ] {
            if out.iter().any(|w| w.role == role) || !tell_holds(state, role) {
                continue;
            }
            let actor = state
                .fixed_identity(role)
                .cloned()
                .unwrap_or_else(|| fallback.clone());
            out.push(Wanted {
                role: role.into(),
                task: None,
                actor,
            });
        }
    }
    // A held person joins the wanted set — the panel's explicit
    // `role.reopen` — whether or not the studio still runs, and even
    // when its task is done. A role a `role.restart` still owes is
    // wanted the same way: the stage stops it and starts it again
    // under the same identity. One already listed stays one entry; a
    // role with no recorded actor has nothing to launch.
    for (key, r) in &state.roles {
        if !(r.held || r.restart.is_some()) || out.iter().any(|w| w.role == *key) {
            continue;
        }
        let Some(actor) = &r.actor else {
            continue;
        };
        out.push(Wanted {
            role: key.clone(),
            task: actor_task(key).map(String::from),
            actor: actor.clone(),
        });
    }
    out.sort_by(|a, b| {
        rank(&a.role)
            .cmp(&rank(&b.role))
            .then(task_key(state, a.task.as_deref()).cmp(&task_key(state, b.task.as_deref())))
    });
    out
}

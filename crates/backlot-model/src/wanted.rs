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
    state
        .messages
        .iter()
        .rev()
        .find(|m| matches!(m.purpose.as_deref(), Some("plan") | Some("dispatch")))
        .is_some_and(|m| m.purpose.as_deref() == Some("plan"))
}

/// The roles the ledger wants live, in slot order. A finished
/// studio wants nobody on its own — the held pass still re-launches
/// a person an explicit `role.reopen` held.
pub fn wanted_roles(state: &StudioState) -> Vec<Wanted> {
    let Some(cfg) = &state.studio else {
        return Vec::new();
    };
    let mut out: Vec<Wanted> = Vec::new();
    if state.finished.is_none() {
        out.push(Wanted {
            role: "producer".into(),
            task: None,
            actor: cfg.roles.producer.clone(),
        });
        out.push(Wanted {
            role: "director".into(),
            task: None,
            actor: cfg.roles.director.clone(),
        });
        if writer_wanted(state) {
            out.push(Wanted {
                role: "writer".into(),
                task: None,
                actor: cfg.roles.writer.clone(),
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
    // A held person joins the wanted set — the panel's explicit
    // `role.reopen` — whether or not the studio still runs, and even
    // when its task is done. One already listed stays one entry; a
    // held role with no actor has nothing to launch.
    for (key, r) in &state.roles {
        if !r.held || out.iter().any(|w| w.role == *key) {
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

//! Desired roles versus live terminals, as a pure function the stage
//! calls each tick. It reads the reduced ledger state plus a
//! name → address view of live terminals and returns the ordered
//! actions — never a side effect. Stops come before starts so a recast
//! frees the terminal name before the new person runs.

use std::collections::{BTreeMap, BTreeSet};

use backlot_model::event::ActorIdentity;
use backlot_model::state::{RoleLifecycle, StudioState, actor_task};

/// Which fixed role a terminal plays (actors key on the task).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleKind {
    Producer,
    Director,
    Writer,
    Actor,
}

impl RoleKind {
    /// The `BACKLOT_ROLE` a terminal of this kind gets.
    pub fn name(self) -> &'static str {
        match self {
            RoleKind::Producer => "producer",
            RoleKind::Director => "director",
            RoleKind::Writer => "writer",
            RoleKind::Actor => "actor",
        }
    }
}

/// A `terminal.run` plus a `role.started` record.
#[derive(Debug, Clone, PartialEq)]
pub struct Start {
    /// Ledger role key: `producer`, `director`, `writer` or
    /// `actor-<task>-c<n>`.
    pub role: String,
    pub kind: RoleKind,
    /// The task an actor works on.
    pub task: Option<String>,
    /// Terminal name, `bl-<studio>-<role>` or `bl-<studio>-<task>`.
    pub terminal: String,
    /// Who runs in it: the studio role identity or the cast.
    pub actor: ActorIdentity,
    /// The studio repo — the terminal's working directory.
    pub repo: String,
    /// The session a held person is reopened into; `None` for a
    /// fresh start.
    pub resume: Option<String>,
}

/// One action the stage carries out, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Append `role.lost`: a live role's terminal is gone with no
    /// `role.closed`. Lost roles are not relaunched (S6 recovers).
    Lost { role: String, reason: String },
    /// Append `role.closed` (unless `closed` is already recorded —
    /// that is a crashed stop finishing), then `terminal.kill` and
    /// `terminal.rm`. An already-absent terminal is tolerated.
    Stop {
        role: String,
        terminal: String,
        address: String,
        /// Whether a `role.closed` is still owed.
        close: bool,
        reason: String,
    },
    /// `terminal.kill` + `terminal.rm` for a `bl-` terminal no role
    /// entry explains — nothing to record.
    DropTerminal { terminal: String, address: String },
    /// Record `role.started` for a terminal that already exists
    /// (the stage died between `terminal.run` and the append). The
    /// runner's session id is filled in when the action executes.
    Adopt {
        role: String,
        terminal: String,
        address: String,
    },
    /// `terminal.run` then `role.started`.
    Start(Start),
}

/// One live named terminal as the label view sees it. `argv` and
/// `cwd` let the watch pass read a chosen session id back and check
/// the folder a trust dialog guards.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveTerminal {
    pub name: String,
    pub address: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub labels: BTreeMap<String, String>,
}

/// This studio's live terminals by name: only the ones labelled
/// `backlot.studio=<studio>`. The name never confers ownership — a
/// hand-named `bl-acme-x` or another studio's `bl-acme-shop-*` lands
/// in no view but its labelled studio's, so it is never stopped,
/// adopted or dropped here.
pub fn studio_view(studio: &str, live: &[LiveTerminal]) -> BTreeMap<String, String> {
    live.iter()
        .filter(|t| t.labels.get("backlot.studio").is_some_and(|s| s == studio))
        .map(|t| (t.name.clone(), t.address.clone()))
        .collect()
}

/// `bl-<studio>-<role>` for fixed roles, `bl-<studio>-<task>` for
/// `actor-<task>-c<n>` keys; `None` for anything else.
pub fn terminal_name(studio: &str, role_key: &str) -> Option<String> {
    match role_key {
        "producer" | "director" | "writer" => Some(format!("bl-{studio}-{role_key}")),
        _ => {
            let task = role_key.strip_prefix("actor-")?.rsplit_once("-c")?.0;
            if task.is_empty() {
                None
            } else {
                Some(format!("bl-{studio}-{task}"))
            }
        }
    }
}

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
fn desired(studio: &str, state: &StudioState) -> Vec<Start> {
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
            want.resume = r.session_id.clone();
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
            resume: Some(session.clone()),
        });
    }
    out
}

/// The actions that move the live view toward the ledger's desired
/// one. `live` is name → hc address for the terminals this studio
/// owns — the caller builds it with [`studio_view`], so every name
/// here is already the studio's by label.
pub fn reconcile(
    studio: &str,
    state: &StudioState,
    live: &BTreeMap<String, String>,
) -> Vec<Action> {
    let wanted = desired(studio, state);
    let wanted_keys: BTreeSet<&str> = wanted.iter().map(|w| w.role.as_str()).collect();
    // Whether any recorded role's key resolves to this terminal name.
    let owned = |name: &str, except: &str| {
        state
            .roles
            .keys()
            .any(|key| key != except && terminal_name(studio, key).as_deref() == Some(name))
    };

    let mut stops = Vec::new();
    let mut records = Vec::new();
    let mut starts = Vec::new();

    // Existing role entries first.
    for (key, r) in &state.roles {
        let Some(name) = terminal_name(studio, key) else {
            continue;
        };
        let term = live.get(&name).cloned();
        match r.lifecycle() {
            RoleLifecycle::Live => {
                if wanted_keys.contains(key.as_str()) {
                    if term.is_none() {
                        records.push(Action::Lost {
                            role: key.clone(),
                            reason: "terminal gone".into(),
                        });
                    }
                } else if let Some(address) = term {
                    stops.push(Action::Stop {
                        role: key.clone(),
                        terminal: name,
                        address,
                        close: true,
                        reason: "no longer desired".into(),
                    });
                } else {
                    // Undesired but recorded live with the terminal
                    // already gone — a loss, not a stop.
                    records.push(Action::Lost {
                        role: key.clone(),
                        reason: "terminal gone".into(),
                    });
                }
            }
            RoleLifecycle::Closed => {
                // A crashed stop finishing — but only for this role's
                // own recorded terminal. The same name at another
                // address belongs to whoever came after (a recast
                // reuses the name): leave that terminal alone.
                if let Some(address) = term.filter(|a| r.terminal.as_ref() == Some(a)) {
                    stops.push(Action::Stop {
                        role: key.clone(),
                        terminal: name,
                        address,
                        close: false,
                        reason: String::new(),
                    });
                }
            }
            // Lost roles are not relaunched (S6) and their terminals,
            // if any, are orphans handled below.
            RoleLifecycle::Lost => {}
        }
    }

    // Desired roles.
    for want in wanted {
        match state.roles.get(&want.role) {
            // Live and its terminal is up: nothing to do.
            Some(r) if r.lifecycle() == RoleLifecycle::Live => {}
            // Lost is not relaunched.
            Some(r) if r.lifecycle() == RoleLifecycle::Lost => {}
            // Closed comes back when desired again (the writer on the
            // next `plan request`), as a new `role.started`.
            Some(_) => starts.push(Action::Start(want)),
            None => {
                if let Some(address) = live.get(&want.terminal) {
                    // A terminal under that name is up. If another
                    // role entry owns the name, it is being stopped
                    // above; otherwise it is this role's own — the
                    // stage died between run and record: adopt it.
                    if owned(&want.terminal, &want.role) {
                        starts.push(Action::Start(want));
                    } else {
                        records.push(Action::Adopt {
                            role: want.role,
                            terminal: want.terminal,
                            address: (*address).clone(),
                        });
                    }
                } else {
                    starts.push(Action::Start(want));
                }
            }
        }
    }

    // Strays: terminals in this studio's view that no role entry
    // explains and nothing adopted this round.
    for (name, address) in live {
        let adopted = records.iter().any(|a| match a {
            Action::Adopt { terminal, .. } => terminal == name,
            _ => false,
        });
        if owned(name, "") || adopted {
            continue;
        }
        stops.push(Action::DropTerminal {
            terminal: name.clone(),
            address: address.clone(),
        });
    }

    let mut actions = records;
    actions.extend(stops);
    actions.extend(starts);
    actions
}

#[cfg(test)]
mod reopen_tests;
#[cfg(test)]
mod tests;

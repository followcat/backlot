//! Desired roles versus live terminals, as a pure function the stage
//! calls each tick. It reads the reduced ledger state plus a
//! name → address view of live terminals and returns the ordered
//! actions — never a side effect. Stops come before starts so a recast
//! frees the terminal name before the new person runs.

use std::collections::{BTreeMap, BTreeSet};

use backlot_model::event::ActorIdentity;
use backlot_model::state::{RoleLifecycle, StudioState};

mod budget;
mod desired;
mod recover;

/// Which fixed role a terminal plays (actors key on the task).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleKind {
    Producer,
    Director,
    Writer,
    Actor,
}

impl RoleKind {
    /// The kind's prose name — the prompt's `{role}`; `BACKLOT_ROLE`
    /// is the ledger key `start.role`, not this.
    pub fn name(self) -> &'static str {
        match self {
            RoleKind::Producer => "producer",
            RoleKind::Director => "director",
            RoleKind::Writer => "writer",
            RoleKind::Actor => "actor",
        }
    }
}

/// A start into an existing runner session, and why — the reason
/// lands on the `role.resumed` event (`reopened` for a held person,
/// `recovered` for a lost role the stage brings back).
#[derive(Debug, Clone, PartialEq)]
pub struct Resume {
    pub session: String,
    pub reason: &'static str,
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
    /// The session to resume and why; `None` for a fresh start.
    pub resume: Option<Resume>,
}

/// One action the stage carries out, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Append `role.lost`: a live role's terminal is gone with no
    /// `role.closed`.
    Lost { role: String, reason: String },
    /// Append `attention.raised` — the stage dedups to one open
    /// attention per role and reason at record time.
    Attention {
        role: String,
        reason: String,
        detail: String,
    },
    /// Append `attention.cleared` for the given open attention.
    ClearAttention { target: u64, reason: String },
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
    /// Append `role.resumed` with reason `recovered` for a lost role
    /// whose terminal is already live — no `terminal.run`, just the
    /// record so the `recovered` notice goes out.
    Resumed {
        role: String,
        address: String,
        session: String,
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

/// The role keys the ledger wants live right now — the nudge pass
/// clears `owed` attention a role no longer desired still holds.
pub(crate) fn wanted(studio: &str, state: &StudioState) -> BTreeSet<String> {
    desired::desired(studio, state)
        .into_iter()
        .map(|s| s.role)
        .collect()
}

/// The actions that move the live view toward the ledger's desired
/// one. `live` is name → hc address for the terminals this studio
/// owns — the caller builds it with [`studio_view`], so every name
/// here is already the studio's by label. `now_ms` feeds the lost
/// role's resume guard; the function stays pure.
pub fn reconcile(
    studio: &str,
    state: &StudioState,
    live: &BTreeMap<String, String>,
    now_ms: u64,
) -> Vec<Action> {
    let wanted = desired::desired(studio, state);
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
    // Attention seqs already given a clear this pass — one per pass.
    let mut cleared = BTreeSet::new();

    // Existing role entries first.
    for (key, r) in &state.roles {
        if let Some(name) = terminal_name(studio, key) {
            let term = live.get(&name).cloned();
            match r.lifecycle() {
                RoleLifecycle::Live => {
                    if wanted_keys.contains(key.as_str()) {
                        if term.is_none() {
                            recover::lost_records(state, key, &mut records, &mut cleared);
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
                        recover::lost_records(state, key, &mut records, &mut cleared);
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
                // A wanted lost role comes back into its recorded
                // session; without one — or once the loss guard trips —
                // it gets a `cannot resume` attention instead. An
                // unwanted lost role needs nothing. A live terminal
                // under the role's name needs no `terminal.run` — the
                // record alone rejoins it, checked before the guard.
                RoleLifecycle::Lost => {
                    if wanted_keys.contains(key.as_str()) {
                        let want = wanted
                            .iter()
                            .find(|w| w.role == *key)
                            .expect("a wanted key has a Start");
                        recover::lost_actions(
                            key,
                            r,
                            want,
                            term.as_ref(),
                            now_ms,
                            &mut records,
                            &mut starts,
                        );
                    }
                }
            }
        }
        // An open `cannot resume` clears once the role is live again
        // or no longer wanted — after any `role.lost` above took its
        // own clears, so a seq clears once per pass.
        recover::cannot_resume_clear(
            key,
            r,
            state,
            wanted_keys.contains(key.as_str()),
            &mut records,
            &mut cleared,
        );
    }

    // Desired roles.
    for want in wanted {
        match state.roles.get(&want.role) {
            // Live and its terminal is up: nothing to do.
            Some(r) if r.lifecycle() == RoleLifecycle::Live => {}
            // A lost role's resume — or its `cannot resume` — was
            // decided in the role pass above; no fresh start here.
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

    // Task budgets: an over-budget task raises once; a terminal one
    // clears its raise `done`.
    budget::actions(state, now_ms, &mut records, &mut cleared);

    // Runner limits (S9e): a held start waits — no event, retried
    // next tick. `limits::held` reads the ledger the same way the
    // views do, so a held role shows `waiting for a <runner> slot`
    // until a slot frees.
    let held = backlot_model::limits::held(state);
    starts.retain(|a| match a {
        Action::Start(w) => !held.contains_key(&w.role),
        _ => true,
    });

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
mod budget_tests;
#[cfg(test)]
mod limits_tests;
#[cfg(test)]
mod recover_tests;
#[cfg(test)]
mod reopen_tests;
#[cfg(test)]
mod tests;

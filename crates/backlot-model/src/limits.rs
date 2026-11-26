//! Per-studio limits: the cast file's `[limits]` table caps how many
//! live roles one runner may carry in this studio, and the cast's
//! `max_actors` caps how many actors play at once — fixed roles and
//! actors alike count on a runner, a role being started counts too,
//! while the actor cap counts actors alone (the producer, the director
//! and the writer never fill an actor slot). Two studios on one
//! account each count only their own.
//!
//! `held` is pure over the reduced state: `backlot status`, the
//! panel's summary and `reconcile` all read the same answer, so a
//! held start is retried every tick and starts the tick a slot
//! frees — nothing is recorded for the wait.

use std::collections::BTreeMap;

use crate::state::{RoleLifecycle, StudioState, actor_task};
use crate::wanted::wanted_roles;

/// Why a wanted role's start is held back — the words `backlot
/// status`, the panel and the TUI show beside the role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Held {
    /// Its runner's `[limits]` cap is full: `waiting for a <runner>
    /// slot`.
    Runner(String),
    /// The studio's `max_actors` cap is full: `waiting for an actor
    /// slot`.
    Actor,
}

impl Held {
    /// The waiting line a held role shows in `backlot status`, the
    /// panel and the TUI.
    pub fn waiting_text(&self) -> String {
        match self {
            Held::Runner(runner) => format!("waiting for a {runner} slot"),
            Held::Actor => "waiting for an actor slot".to_string(),
        }
    }
}

/// The roles that hold an actor slot: live roles whose key names a
/// task. A fixed role never counts.
pub fn live_actors(state: &StudioState) -> usize {
    state
        .roles
        .iter()
        .filter(|(role, r)| r.lifecycle() == RoleLifecycle::Live && actor_task(role).is_some())
        .count()
}

/// The studio's actor cap — `max_actors` from the newest cast, or
/// `default_max_actors` before a studio event.
pub fn actor_cap(state: &StudioState) -> u32 {
    state
        .studio
        .as_ref()
        .map_or_else(crate::event::default_max_actors, |c| c.max_actors)
}

/// The wanted roles whose start is held back, mapped to why — the
/// roles `reconcile` holds back and the views name.
pub fn held(state: &StudioState) -> BTreeMap<String, Held> {
    let Some(cfg) = &state.studio else {
        return BTreeMap::new();
    };
    // Live roles hold their runner slots now — recorded live counts,
    // live terminal or not (the ledger's view is what every caller
    // sees).
    let mut occ: BTreeMap<String, usize> = BTreeMap::new();
    for r in state.roles.values() {
        if r.lifecycle() == RoleLifecycle::Live
            && let Some(a) = &r.actor
        {
            *occ.entry(a.runner.clone()).or_default() += 1;
        }
    }
    // A live actor holds an actor slot; the actor cap counts actors
    // only, never the fixed roles.
    let max_actors = cfg.max_actors as usize;
    let mut actors = live_actors(state);
    // Walk the wanted set in slot order: a live one is already
    // counted, a waiting one either takes its slots — being started
    // counts too — or names the runner or the actor cap it waits on.
    let mut held = BTreeMap::new();
    for w in wanted_roles(state) {
        let already_live = state
            .roles
            .get(&w.role)
            .is_some_and(|r| r.lifecycle() == RoleLifecycle::Live);
        if already_live {
            continue;
        }
        let runner = w.actor.runner;
        let runner_free = cfg
            .limits
            .get(&runner)
            .is_none_or(|cap| *occ.get(runner.as_str()).unwrap_or(&0) < *cap as usize);
        if !runner_free {
            held.insert(w.role, Held::Runner(runner));
            continue;
        }
        let is_actor = actor_task(&w.role).is_some();
        if is_actor && actors >= max_actors {
            held.insert(w.role, Held::Actor);
            continue;
        }
        *occ.entry(runner).or_default() += 1;
        if is_actor {
            actors += 1;
        }
    }
    held
}

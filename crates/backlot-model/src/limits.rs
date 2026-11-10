//! Per-studio runner limits (S9e): the cast file's `[limits]`
//! table caps how many live roles one runner may carry in this
//! studio — fixed roles and actors alike, and a role being started
//! counts too. Two studios on one account each count only their
//! own.
//!
//! `held` is pure over the reduced state: `backlot status`, the
//! panel's summary and `reconcile` all read the same answer, so a
//! held start is retried every tick and starts the tick a slot
//! frees — nothing is recorded for the wait.

use std::collections::BTreeMap;

use crate::state::{RoleLifecycle, StudioState};
use crate::wanted::wanted_roles;

/// The wanted roles whose start would push their runner past its
/// limit, mapped to the runner name — the roles `reconcile` holds
/// back and the views name `waiting for a <runner> slot`.
pub fn held(state: &StudioState) -> BTreeMap<String, String> {
    let Some(cfg) = &state.studio else {
        return BTreeMap::new();
    };
    // Live roles hold their slots now — recorded live counts, live
    // terminal or not (the ledger's view is what every caller sees).
    let mut occ: BTreeMap<String, usize> = BTreeMap::new();
    for r in state.roles.values() {
        if r.lifecycle() == RoleLifecycle::Live
            && let Some(a) = &r.actor
        {
            *occ.entry(a.runner.clone()).or_default() += 1;
        }
    }
    // Walk the wanted set in slot order: a live one is already
    // counted, a waiting one either takes a slot — being started
    // counts too — or names its runner here.
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
        let free = cfg
            .limits
            .get(&runner)
            .is_none_or(|cap| *occ.get(runner.as_str()).unwrap_or(&0) < *cap as usize);
        if free {
            *occ.entry(runner).or_default() += 1;
        } else {
            held.insert(w.role, runner);
        }
    }
    held
}

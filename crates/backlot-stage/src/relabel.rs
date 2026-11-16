//! Relabelling live role terminals (U2): terminals launched before
//! a studio had its own space carry the old shared `backlot` space
//! and `bl-…` tab names. Each tick the stage rewrites
//! `herdr.space`/`herdr.tab` on the ones it still owns — once per
//! terminal and only while the core proves it can change labels on
//! a live one: a core that cannot keeps them where they launched
//! until their next start.

use std::collections::BTreeMap;

use backlot_model::state::{StudioState, actor_task};

use crate::act;
use crate::reconcile::{self, LiveTerminal};

/// The `herdr.*` patch `term` still needs under the per-studio
/// scheme — `None` when it is already right, is a shell, or is no
/// role of `studio` by both `backlot.*` labels and the ledger: a
/// terminal the studio does not own is never rewritten, and a name
/// never confers the role.
pub(crate) fn wanted_labels(
    studio: &str,
    state: &StudioState,
    term: &LiveTerminal,
) -> Option<BTreeMap<String, String>> {
    if term.labels.contains_key("backlot.shell")
        || term.labels.get("backlot.studio").map(String::as_str) != Some(studio)
    {
        return None;
    }
    let role = term.labels.get("backlot.role")?.as_str();
    // Label and ledger must name the same role of this studio —
    // a foreign `backlot.*` terminal keeps its place.
    if reconcile::terminal_name(studio, role).as_deref() != Some(term.name.as_str())
        || !state.roles.contains_key(role)
    {
        return None;
    }
    let tab = act::role_tab(state, role, actor_task(role));
    let mut patch = BTreeMap::new();
    if term.labels.get("herdr.space").map(String::as_str) != Some(studio) {
        patch.insert("herdr.space".to_string(), studio.to_string());
    }
    if term.labels.get("herdr.tab").map(String::as_str) != Some(tab.as_str()) {
        patch.insert("herdr.tab".to_string(), tab);
    }
    (!patch.is_empty()).then_some(patch)
}

#[cfg(test)]
mod tests;

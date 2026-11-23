//! Studio mode's client-local state (U1): the mode latch, the current
//! studio and the person last focused in each. Everything here is
//! memory only — nothing is stored, and a publish recomputes the
//! latch from the last good `backlot` documents.

use std::collections::BTreeMap;

use backlot_model::summary::{RoleSummary, StudioSummary};

pub(crate) mod glyphs;

use crate::panel::data::MachineSummary;

/// A studio's stable identity across machines: the machine's name
/// (`None` is this core) plus the document's studio id. The same id
/// on two machines is two studios — everything user-facing keys by
/// this pair.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct StudioKey {
    /// The machine's name, or `None` for this core.
    pub machine: Option<String>,
    /// The studio id the document carries.
    pub id: String,
}

impl StudioKey {
    /// `id` on this core.
    pub(crate) fn local(id: &str) -> StudioKey {
        StudioKey {
            machine: None,
            id: id.into(),
        }
    }
}

/// A bare id reads as this core's key — the common local-studio
/// spelling in tests and call sites.
impl From<&str> for StudioKey {
    fn from(id: &str) -> StudioKey {
        StudioKey::local(id)
    }
}

/// `key == "id"`: this core's `id` — a remote studio never equals a
/// bare id.
impl PartialEq<str> for StudioKey {
    fn eq(&self, id: &str) -> bool {
        self.machine.is_none() && self.id == id
    }
}

/// The `&str` spelling of the same comparison.
impl PartialEq<&str> for StudioKey {
    fn eq(&self, id: &&str) -> bool {
        self == *id
    }
}

/// The client-local studio state: whether studio mode is on, which
/// studio is current, the person each studio last focused and the
/// cast's `others` expansion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StudioFocus {
    Studio(StudioKey),
    Role(StudioKey, String),
}

#[derive(Debug, Default)]
pub(crate) struct Studio {
    /// The studio-layout latch (U2): on whenever the client is not
    /// `--plain` — `sync_studio` owns it, so a missing, malformed or
    /// foreign-schema document never changes it and the cast (with
    /// its `others` row) shows even with zero studios.
    pub active: bool,
    /// The current studio — the studio whose person owns the focused
    /// terminal, or the last such studio while the focus is
    /// elsewhere. At the start it is the first studio in the
    /// aggregate's order.
    pub current: Option<StudioKey>,
    /// Independent studio/role cursor for `Alt+arrows` and `Ctrl+B m`.
    pub focus: Option<StudioFocus>,
    /// `(machine, studio) → role`: the person last focused in each
    /// studio, so returning lands on the same person.
    pub remembered: BTreeMap<StudioKey, String>,
    /// The cast's `others` row, expanded in place.
    pub others_expanded: bool,
    /// W1: the machines whose `finished (n)` group is unfolded — keyed
    /// by machine (`None` is this core).
    pub finished_open: std::collections::BTreeSet<Option<String>>,
    /// The cast section's scroll offset — its own, so the agents
    /// list's scroll survives a mode flip either way.
    pub(crate) cast_scroll: usize,
    /// The lot latch — the pill's counts and the focused workspace's
    /// studio header, rebuilt by `sync_studio`.
    pub(crate) lot: crate::chrome::lot::Lot,
    last_focused_terminal: Option<String>,
}

impl Studio {
    /// Target the studio row itself without changing `App::focused`.
    pub(crate) fn set_studio_focus(&mut self, key: StudioKey) {
        self.current = Some(key.clone());
        self.focus = Some(StudioFocus::Studio(key));
    }

    /// Force the next sync pass to re-evaluate `App::focused` even if it
    /// points to the same terminal pane as before.
    pub(crate) fn clear_focus_latch(&mut self) {
        self.last_focused_terminal = None;
    }

    /// Studio currently owning `StudioFocus::Studio(key)` (not `Role`).
    pub(crate) fn focused_studio(&self) -> Option<&StudioKey> {
        match &self.focus {
            Some(StudioFocus::Studio(key)) => Some(key),
            _ => None,
        }
    }

    /// Sync against the last good summaries and the focused terminal.
    /// No machine's document at all keeps every latch. The `active`
    /// latch itself is `sync_studio`'s (`!plain`), not the
    /// documents'.
    #[cfg(test)]
    pub(crate) fn sync(&mut self, machines: &[MachineSummary], focused: Option<&str>) {
        self.sync_with_shell(machines, focused, None);
    }

    /// Sync against summaries, the focused terminal, and any resolved
    /// studio-shell ownership for `focused`.
    pub(crate) fn sync_with_shell(
        &mut self,
        machines: &[MachineSummary],
        focused: Option<&str>,
        shell_key: Option<StudioKey>,
    ) {
        if machines.is_empty() {
            return;
        }
        let studios = crate::panel::data::studios(machines);
        let focus_changed = focused != self.last_focused_terminal.as_deref();
        if focus_changed {
            self.last_focused_terminal = focused.map(str::to_owned);
        }
        let matched = focused.and_then(|terminal| {
            studios
                .iter()
                .find_map(|s| {
                    s.studio
                        .roles
                        .iter()
                        .find(|role| role.terminal.as_deref() == Some(terminal))
                        .map(|role| (s.key(), role.role.clone(), terminal.to_owned()))
                })
                .or_else(|| {
                    shell_key
                        .clone()
                        .filter(|key| studios.iter().any(|s| &s.key() == key))
                        .map(|key| (key, terminal.to_owned(), terminal.to_owned()))
                })
        });
        if focus_changed {
            if let Some((key, member, terminal)) = matched {
                self.current = Some(key.clone());
                self.remembered.insert(key.clone(), member);
                self.focus = Some(StudioFocus::Role(key, terminal));
            } else if matches!(self.focus, Some(StudioFocus::Role(..))) {
                self.focus = None;
            }
        } else if let Some(StudioFocus::Studio(key)) = &self.focus {
            if studios.iter().any(|s| &s.key() == key) {
                self.current = Some(key.clone());
            } else {
                self.focus = studios.first().map(|s| StudioFocus::Studio(s.key()));
            }
        } else if let Some(StudioFocus::Role(key, _)) = &self.focus
            && !studios.iter().any(|s| &s.key() == key)
        {
            self.focus = None;
        }
        // A current studio that left every machine's document falls
        // back to the aggregate's order, like the start does.
        if self
            .current
            .as_ref()
            .is_none_or(|key| !studios.iter().any(|s| &s.key() == key))
        {
            self.current = studios.first().map(|s| s.key());
        }
    }

    /// The current studio's index in the aggregate's order — `None`
    /// only when no machine's document lists a studio.
    pub(crate) fn current_index(&self, machines: &[MachineSummary]) -> Option<usize> {
        let studios = crate::panel::data::studios(machines);
        if studios.is_empty() {
            return None;
        }
        self.current
            .as_ref()
            .and_then(|key| studios.iter().position(|s| &s.key() == key))
            .or(Some(0))
    }
}

/// A person's terminal while live — `None` for lost, closed and
/// unseated (waiting) people, the ones navigation skips.
pub(crate) fn live_terminal(role: &RoleSummary) -> Option<&str> {
    match role.state.as_str() {
        "live" => role.terminal.as_deref(),
        _ => None,
    }
}

/// A studio's people in cast order: the open ones in the summary's
/// order, then the closed ones after them.
pub(crate) fn cast_order(studio: &StudioSummary) -> Vec<&RoleSummary> {
    studio
        .roles
        .iter()
        .filter(|role| role.state != "closed")
        .chain(studio.roles.iter().filter(|role| role.state == "closed"))
        .collect()
}

/// A studio's `!<n>` count: roles carrying an attention.
pub(crate) fn attention_count(studio: &StudioSummary) -> usize {
    studio
        .roles
        .iter()
        .filter(|role| role.attention.is_some())
        .count()
}

/// A studio's working count for the collapsed strip.
#[cfg(test)]
pub(crate) fn working_count(studio: &StudioSummary) -> usize {
    studio
        .roles
        .iter()
        .filter(|role| role.work == Some(backlot_model::summary::Work::Working))
        .count()
}

#[cfg(test)]
#[path = "studio_tests.rs"]
mod tests;

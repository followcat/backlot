//! Studio mode's client-local state (U1): the mode latch, the current
//! studio and the person last focused in each. Everything here is
//! memory only — nothing is stored, and a publish recomputes the
//! latch from the last good `backlot` document.

use std::collections::BTreeMap;

use backlot_model::summary::{RoleSummary, StudioSummary, Summary};

/// The client-local studio state: whether studio mode is on, which
/// studio is current, the person each studio last focused and the
/// cast's `others` expansion.
#[derive(Debug, Default)]
pub(crate) struct Studio {
    /// True while the last good document lists at least one studio.
    /// Only a good document flips the latch: a missing, malformed or
    /// foreign-schema read never turns it off, and only a good one
    /// with zero studios does.
    pub active: bool,
    /// The current studio's id — the studio whose person owns the
    /// focused terminal, or the last such studio while the focus is
    /// elsewhere. At the start it is the first studio in document
    /// order.
    pub current: Option<String>,
    /// `studio id → role`: the person last focused in each studio, so
    /// returning lands on the same person.
    pub remembered: BTreeMap<String, String>,
}

impl Studio {
    /// Sync against the last good summary and the focused terminal.
    /// `None` — a missing or unreadable document — keeps every latch.
    pub(crate) fn sync(&mut self, summary: Option<&Summary>, focused: Option<&str>) {
        let Some(summary) = summary else { return };
        self.active = !summary.studios.is_empty();
        // A focused terminal owned by a studio person moves the
        // current pointer and remembers the person there.
        if let Some((studio, role)) = studio_of_terminal(summary, focused.unwrap_or_default()) {
            self.current = Some(studio.id.clone());
            self.remembered.insert(studio.id.clone(), role.role.clone());
        }
        // A current studio that left the document falls back to
        // document order, like the start does.
        if self
            .current
            .as_ref()
            .is_none_or(|id| !summary.studios.iter().any(|s| &s.id == id))
        {
            self.current = summary.studios.first().map(|s| s.id.clone());
        }
    }

    /// The current studio's index in document order — `None` only
    /// when the document lists no studios.
    pub(crate) fn current_index(&self, summary: &Summary) -> Option<usize> {
        if summary.studios.is_empty() {
            return None;
        }
        self.current
            .as_ref()
            .and_then(|id| summary.studios.iter().position(|s| &s.id == id))
            .or(Some(0))
    }
}

/// The `(studio, role)` owning a terminal, if any person does.
pub(crate) fn studio_of_terminal<'a>(
    summary: &'a Summary,
    terminal: &str,
) -> Option<(&'a StudioSummary, &'a RoleSummary)> {
    summary.studios.iter().find_map(|studio| {
        studio
            .roles
            .iter()
            .find(|role| role.terminal.as_deref() == Some(terminal))
            .map(|role| (studio, role))
    })
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

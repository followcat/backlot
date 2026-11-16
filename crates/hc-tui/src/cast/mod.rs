//! The studio cast (U1): in studio mode the sidebar's agents section
//! becomes the current studio's people — two rows each, the focused
//! person marked, closed people last — and an `others` row that
//! expands into herdr's own agent rows. Everything the renderer needs
//! is a borrow of the summary plus the terminal inventory's titles.

mod draw;
pub(crate) mod rows;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

use std::collections::BTreeMap;

use backlot_model::summary::{StudioSummary, Summary};
use ratatui::layout::Rect;

use crate::chrome::ClientShellConfig;
use crate::chrome::agents::{AgentRow, agent_rows};
use crate::chrome::snapshot::ClientShellSnapshot;
use crate::chrome::title_text::stripped_terminal_title;

pub(crate) use draw::render_cast;

/// What the cast draws with, built per frame: the current studio, its
/// position, the focused terminal and the stripped terminal titles.
pub(crate) struct CastView<'a> {
    /// The current studio.
    pub(crate) studio: &'a StudioSummary,
    /// Its position in document order (0-based) and the studio count —
    /// the header's `<n>/<total>`.
    pub(crate) index: usize,
    pub(crate) total: usize,
    /// The focused terminal — its owner gets the active rows.
    pub(crate) focused_terminal: Option<&'a str>,
    /// `terminal → stripped title` for a person's second row.
    pub(crate) titles: BTreeMap<String, Option<String>>,
    /// Herdr's agents that are no studio person — the `others` row's
    /// count and, expanded, its rows.
    pub(crate) others: Vec<AgentRow>,
    /// Whether `others` is expanded in place.
    pub(crate) others_expanded: bool,
    /// The working-mark spinner frame (~120 ms ticks).
    pub(crate) spin: usize,
}

impl<'a> CastView<'a> {
    /// Build the frame's cast: the studio `current` points at (or the
    /// first in document order when it does not resolve), the people
    /// in cast order and every herdr agent that no studio's person
    /// seats. `None` outside studio mode — no summary, no studios or
    /// no resolvable current studio.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build(
        summary: Option<&'a Summary>,
        snapshot: &'a ClientShellSnapshot,
        view: &'a hc_proto::watch::View,
        current: Option<&str>,
        config: &'a ClientShellConfig,
        others_expanded: bool,
        focused: Option<&'a str>,
        spin: usize,
    ) -> Option<CastView<'a>> {
        let summary = summary?;
        if summary.studios.is_empty() {
            return None;
        }
        let index = current
            .and_then(|id| summary.studios.iter().position(|s| s.id == id))
            .unwrap_or(0);
        let studio = &summary.studios[index];
        // Every person of every studio is out of `others` — its rows
        // are herdr's agents minus the whole cast of the document.
        let seated: std::collections::BTreeSet<&str> = summary
            .studios
            .iter()
            .flat_map(|s| s.roles.iter())
            .filter_map(|role| role.terminal.as_deref())
            .collect();
        let others = agent_rows(snapshot, config, None)
            .into_iter()
            .filter(|row| !seated.contains(row.pane_id.as_str()))
            .collect();
        let titles = studio
            .roles
            .iter()
            .filter_map(|role| role.terminal.clone())
            .map(|terminal| {
                let title = view
                    .inventory
                    .get(&terminal)
                    .and_then(|term| stripped_terminal_title(&term.title));
                (terminal, title)
            })
            .collect();
        Some(CastView {
            studio,
            index,
            total: summary.studios.len(),
            focused_terminal: focused,
            titles,
            others,
            others_expanded,
            spin,
        })
    }
}

/// The spaces:cast split in studio mode — the spaces section takes
/// only the rows it needs, at most a third of the sidebar height, and
/// the cast gets the rest. Replaces `expanded_sidebar_sections`'s
/// draggable split while the cast is up.
pub(crate) fn sections(area: Rect, spaces_needed: u16) -> (Rect, Rect) {
    let content = Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height);
    if content.is_empty() {
        return (Rect::default(), Rect::default());
    }
    let workspace = spaces_needed
        .min((content.height / 3).max(1))
        .clamp(1, content.height.saturating_sub(1).max(1));
    (
        Rect::new(content.x, content.y, content.width, workspace),
        Rect::new(
            content.x,
            content.y + workspace,
            content.width,
            content.height - workspace,
        ),
    )
}

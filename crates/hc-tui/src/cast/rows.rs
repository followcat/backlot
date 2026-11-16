//! The cast's row model: the current studio's people (open first,
//! closed last — `studio::cast_order`), then the `others` toggle and,
//! expanded, herdr's own agent rows.

use backlot_model::summary::RoleSummary;

use super::{CastView, ShellRow};
use crate::chrome::agents::AgentRow;
use crate::studio;

/// One cast-list entry — a person (two display lines), the `shells`
/// group header or one of its shells, the `others` toggle row, or an
/// expanded herdr agent row.
pub(crate) enum CastRow<'a> {
    /// A studio person: `role`'s two display lines.
    Person {
        role: &'a RoleSummary,
        /// The person's terminal owns the focus — the active-row
        /// background and the accent `▌`.
        focused: bool,
    },
    /// ` shells` — the group header for the studio's own shells.
    Shells,
    /// A studio shell — `shell`'s two display lines, laid out like a
    /// person's.
    Shell {
        shell: &'a ShellRow,
        /// The shell's terminal owns the focus.
        focused: bool,
    },
    /// `others <n> ▸` — the toggle for agents that are no studio
    /// person.
    Others { count: usize, expanded: bool },
    /// An `others` agent drawn with herdr's own rows.
    Agent(&'a AgentRow),
}

impl CastRow<'_> {
    /// Display lines — two per person or shell, one for the group
    /// headers, the agent row's own token rows.
    pub(crate) fn lines(&self) -> u16 {
        match self {
            CastRow::Person { .. } | CastRow::Shell { .. } => 2,
            CastRow::Shells | CastRow::Others { .. } => 1,
            CastRow::Agent(row) => row.rows.len().max(1).min(u16::MAX as usize) as u16,
        }
    }
}

/// The section's rows: cast order first, then the `shells` group,
/// then `others` (and its expanded rows). Two rows per person or
/// shell, no gap anywhere.
pub(crate) fn cast_rows<'a>(view: &'a CastView<'a>) -> Vec<CastRow<'a>> {
    let mut rows = Vec::new();
    for role in view
        .studio
        .map(|studio| studio::cast_order(studio))
        .unwrap_or_default()
    {
        rows.push(CastRow::Person {
            role,
            focused: view.focused_terminal.is_some()
                && role.terminal.as_deref() == view.focused_terminal,
        });
    }
    if !view.shells.is_empty() {
        rows.push(CastRow::Shells);
        for shell in &view.shells {
            rows.push(CastRow::Shell {
                shell,
                focused: view.focused_terminal == Some(shell.terminal.as_str()),
            });
        }
    }
    if !view.others.is_empty() {
        rows.push(CastRow::Others {
            count: view.others.len(),
            expanded: view.others_expanded,
        });
        if view.others_expanded {
            rows.extend(view.others.iter().map(CastRow::Agent));
        }
    }
    rows
}

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
    /// A divider separating open and closed roles (`┄ closed (n) ┄`).
    ClosedDivider { count: usize },
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
    /// C1: the one dim line a studio-less selection leaves —
    /// `select a studio (Enter on it)`.
    Hint,
    /// W1b: the one dim line the empty cast leaves while no stage runs
    /// — `stage stopped — s to start`.
    Stopped,
    /// An `others` agent drawn with herdr's own rows.
    Agent(&'a AgentRow),
}

impl CastRow<'_> {
    /// Display lines — two per person or shell, one for the group
    /// headers, the agent row's own token rows.
    pub(crate) fn lines(&self) -> u16 {
        match self {
            CastRow::Person { .. } | CastRow::Shell { .. } => 2,
            CastRow::ClosedDivider { .. }
            | CastRow::Shells
            | CastRow::Others { .. }
            | CastRow::Hint
            | CastRow::Stopped => 1,
            CastRow::Agent(row) => row.rows.len().max(1).min(u16::MAX as usize) as u16,
        }
    }

    /// Whether this entry belongs to the focused pane — the target a
    /// focus-change reveal scrolls to.
    pub(crate) fn focused(&self) -> bool {
        match self {
            CastRow::Person { focused, .. } | CastRow::Shell { focused, .. } => *focused,
            CastRow::Agent(row) => row.focused,
            CastRow::ClosedDivider { .. }
            | CastRow::Shells
            | CastRow::Others { .. }
            | CastRow::Hint
            | CastRow::Stopped => false,
        }
    }
}

/// The section's rows: cast order first, then the `shells` group,
/// then `others` (and its expanded rows). Two rows per person or
/// shell, plus a `┄ closed (n) ┄` divider when open and closed roles
/// are both present.
pub(crate) fn cast_rows<'a>(view: &'a CastView<'a>) -> Vec<CastRow<'a>> {
    let mut rows = Vec::new();
    if view.hint {
        rows.push(CastRow::Hint);
    } else if view.server_stopped || view.stopped {
        rows.push(CastRow::Stopped);
    }
    let ordered = view
        .studio
        .map(|studio| studio::cast_order(studio.studio))
        .unwrap_or_default();
    let has_open = ordered.iter().any(|r| r.state != "closed");
    let closed_count = ordered.iter().filter(|r| r.state == "closed").count();
    let mut divider_emitted = false;
    for role in ordered {
        if has_open && closed_count > 0 && !divider_emitted && role.state == "closed" {
            rows.push(CastRow::ClosedDivider {
                count: closed_count,
            });
            divider_emitted = true;
        }
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

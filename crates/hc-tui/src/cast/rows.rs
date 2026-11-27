//! The cast's row model: the current studio's people (open first,
//! closed last — `studio::cast_order`), then the `others` toggle and,
//! expanded, herdr's own agent rows.

use backlot_model::summary::RoleSummary;

use super::{CastView, ShellRow};
use crate::chrome::agents::AgentRow;
use crate::studio::{self, StudioKey};

/// Build `runner · model · effort` for a non-closed role using only
/// fields actually present in the summary (never guessing missing ones).
pub(super) fn role_meta_line(role: &RoleSummary) -> Option<String> {
    if role.state == "closed" {
        return None;
    }
    let parts: Vec<&str> = [&role.runner, &role.model, &role.effort]
        .into_iter()
        .filter_map(|field| field.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// One cast-list entry — a person (one line plus optional runner/model
/// sub-line), the `shells` group header or one of its shells, the
/// `others` toggle row, or an expanded herdr agent row.
pub(crate) enum CastRow<'a> {
    /// A studio person: `role`'s primary row plus optional indented model line.
    Person {
        role: &'a RoleSummary,
        /// The person's terminal owns the focus — the active-row
        /// background and the accent `▌`.
        focused: bool,
        /// Formatted `runner · model · effort` sub-line (`None` on closed
        /// seats, when omitted, or after height-pressure folding).
        meta: Option<String>,
    },
    /// A divider separating open and closed roles (`┄ closed (n) ┄` in plain/herdr chrome).
    ClosedDivider { count: usize },
    /// The one-line collapsed closed-roles summary (`── · N closed · … ▸` in studio chrome).
    ClosedFold {
        studio: StudioKey,
        count: usize,
        tokens: Vec<String>,
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
    /// Display lines — two per open role with a metadata line (one when
    /// closed or folded under height pressure), two per shell, one for
    /// group headers, and the agent row's own token rows.
    pub(crate) fn lines(&self) -> u16 {
        match self {
            CastRow::Person { meta, .. } => 1 + u16::from(meta.is_some()),
            CastRow::Shell { .. } => 2,
            CastRow::ClosedDivider { .. }
            | CastRow::ClosedFold { .. }
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
            | CastRow::ClosedFold { .. }
            | CastRow::Shells
            | CastRow::Others { .. }
            | CastRow::Hint
            | CastRow::Stopped => false,
        }
    }

    /// Whether keyboard-cursor reveal should scroll this entry into view.
    pub(crate) fn reveal_target(&self, view: &CastView<'_>) -> bool {
        if view.region_focused && view.cursor_target.is_some() {
            match self {
                CastRow::Person { role, .. } => matches!(
                    &view.cursor_target,
                    Some(crate::focus::CastTarget::Role { role: r, .. }) if *r == role.role
                ),
                CastRow::ClosedFold { studio, .. } => matches!(
                    &view.cursor_target,
                    Some(crate::focus::CastTarget::ClosedRoles { studio: s }) if s == studio
                ),
                CastRow::Shell { shell, .. } => matches!(
                    &view.cursor_target,
                    Some(crate::focus::CastTarget::Shell { terminal: t, .. }) if *t == shell.terminal
                ),
                CastRow::Others { .. } => {
                    matches!(
                        view.cursor_target,
                        Some(crate::focus::CastTarget::OthersToggle)
                    )
                }
                CastRow::Agent(row) => matches!(
                    &view.cursor_target,
                    Some(crate::focus::CastTarget::OtherAgent(id)) if id == &row.pane_id
                ),
                CastRow::ClosedDivider { .. }
                | CastRow::Shells
                | CastRow::Hint
                | CastRow::Stopped => false,
            }
        } else {
            self.focused()
        }
    }
}

/// Build Cast rows using `view.studio_chrome`.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn cast_rows<'a>(view: &'a CastView<'a>) -> Vec<CastRow<'a>> {
    cast_rows_with(view, view.studio_chrome)
}

/// The section's rows: open people first, then either the collapsed
/// closed-roles fold (`studio_chrome = true`) or the legacy divider plus
/// closed people (`studio_chrome = false`), then `shells` and `others`.
pub(crate) fn cast_rows_with<'a>(view: &'a CastView<'a>, studio_chrome: bool) -> Vec<CastRow<'a>> {
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
    if studio_chrome {
        for role in ordered.iter().copied().filter(|r| r.state != "closed") {
            rows.push(CastRow::Person {
                role,
                focused: view.focused_terminal.is_some()
                    && role.terminal.as_deref() == view.focused_terminal,
                meta: role_meta_line(role),
            });
        }
        if let Some(studio) = view.studio {
            let (count, tokens) = crate::panel::closed_fold_summary(studio.studio);
            if count > 0 {
                rows.push(CastRow::ClosedFold {
                    studio: studio.key(),
                    count,
                    tokens,
                });
            }
        }
    } else {
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
                meta: role_meta_line(role),
            });
        }
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

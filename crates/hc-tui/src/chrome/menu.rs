// Derived from herdr src/client/shell/context_menu.rs, state.rs and
// overlays.rs::render_context_menu (Apache-2.0).
//
//! Right-click context menu: the same item sets, ordering, geometry and
//! highlight styles as the reference. `Machine` rows are an hc-tui
//! extension — the reference has no machine menu (parity.md).

use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::dialogs::{contrast, panel};
use super::palette::Palette;
use super::render::{display_width, put_text};
use super::text::truncate_end;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextMenuAction {
    Rename,
    Close,
    NewWorktree,
    OpenWorktree,
    RemoveWorktree,
    ToggleGroup,
    NewTab,
    RenamePane,
    ClearPaneName,
    SwapWithFocusedPane,
    SplitRight,
    SplitDown,
    Zoom,
    ToggleRightClickPassthrough,
    ClosePane,
    MachineAdd,
    MachineConnect,
    MachineRename,
    MachineRemove,
    Peek,
    SendLine,
    Interrupt,
    MarkRead,
    MarkUnread,
    Keep,
    Park,
    Kill,
    /// A studio-menu row — the index into `StudioMenu::items`.
    Studio(usize),
}

#[derive(Debug)]
pub(crate) enum ContextMenuTarget {
    Workspace {
        workspace_id: String,
        is_git: bool,
        is_linked_worktree: bool,
        has_worktree_children: bool,
        close_group: bool,
        collapsed: bool,
    },
    Tab {
        tab_id: String,
        workspace_id: String,
    },
    Pane {
        pane_id: String,
        workspace_id: String,
        source_pane_id: Option<String>,
        has_manual_label: bool,
        right_click_passthrough: bool,
    },
    /// `None` is the Local row; `Some(name)` a `machine.list` entry.
    Machine { name: Option<String> },
    /// hc-tui extension: the agent row menu (parity.md).
    Agent { pane_id: String },
    /// S3: a dock, cast or Call Sheet studio menu — its item list is
    /// the model's own (`StudioMenu::items`).
    Studio(Box<crate::studio_menu::StudioMenu>),
}

#[derive(Debug)]
pub(crate) struct ContextMenu {
    pub target: ContextMenuTarget,
    pub x: u16,
    pub y: u16,
    pub highlighted: usize,
}

pub(crate) struct ContextMenuItem {
    pub label: Cow<'static, str>,
    pub action: ContextMenuAction,
    /// S3: a dimmed-with-reason row draws dim and does nothing.
    pub dim: Option<String>,
    /// S3: the key that fires it while the menu is open.
    pub key: Option<char>,
    /// S3: `ready_to_finish`'s finish row draws teal.
    pub teal: bool,
}

impl ContextMenu {
    pub(crate) fn title(&self) -> Option<String> {
        match &self.target {
            ContextMenuTarget::Studio(menu) => Some(menu.title()),
            _ => None,
        }
    }

    pub(crate) fn items(&self) -> Vec<ContextMenuItem> {
        use ContextMenuAction as Action;

        let item = |label: &'static str, action| ContextMenuItem {
            label: Cow::Borrowed(label),
            action,
            dim: None,
            key: None,
            teal: false,
        };
        match &self.target {
            ContextMenuTarget::Workspace { is_git: false, .. } => {
                vec![item("Rename", Action::Rename), item("Close", Action::Close)]
            }
            ContextMenuTarget::Workspace {
                is_linked_worktree: false,
                has_worktree_children: false,
                ..
            } => vec![
                item("Rename", Action::Rename),
                item("Close", Action::Close),
                item("New worktree", Action::NewWorktree),
                item("Open worktree...", Action::OpenWorktree),
            ],
            ContextMenuTarget::Workspace {
                is_linked_worktree: true,
                ..
            } => vec![
                item("Rename", Action::Rename),
                item("Close", Action::Close),
                item("Delete worktree checkout...", Action::RemoveWorktree),
            ],
            ContextMenuTarget::Workspace {
                has_worktree_children: true,
                close_group,
                collapsed,
                ..
            } => vec![
                item("Rename", Action::Rename),
                item(
                    if *close_group { "Close group" } else { "Close" },
                    Action::Close,
                ),
                item("New worktree", Action::NewWorktree),
                item("Open worktree...", Action::OpenWorktree),
                item(
                    if *collapsed { "Expand" } else { "Collapse" },
                    Action::ToggleGroup,
                ),
            ],
            ContextMenuTarget::Tab { .. } => vec![
                item("New tab", Action::NewTab),
                item("Rename", Action::Rename),
                item("Close", Action::Close),
            ],
            ContextMenuTarget::Pane {
                source_pane_id,
                has_manual_label,
                right_click_passthrough,
                ..
            } => {
                let mut items = vec![item("Rename pane", Action::RenamePane)];
                if *has_manual_label {
                    items.push(item("Clear pane name", Action::ClearPaneName));
                }
                if source_pane_id.is_some() {
                    items.push(item("Swap with focused pane", Action::SwapWithFocusedPane));
                }
                items.extend([
                    item("Split right", Action::SplitRight),
                    item("Split down", Action::SplitDown),
                    item("Zoom", Action::Zoom),
                    item(
                        if *right_click_passthrough {
                            "Use Herdr right-click menu"
                        } else {
                            "Send right-clicks to pane"
                        },
                        Action::ToggleRightClickPassthrough,
                    ),
                    item("Close pane", Action::ClosePane),
                ]);
                items
            }
            // hc-tui extension: herdr has no machine menu surface.
            ContextMenuTarget::Machine { name: None } => {
                vec![item("Add machine", Action::MachineAdd)]
            }
            ContextMenuTarget::Machine { name: Some(_) } => vec![
                item("Connect", Action::MachineConnect),
                item("Rename machine", Action::MachineRename),
                item("Remove machine", Action::MachineRemove),
            ],
            // hc-tui extension: the reference TUI has no agent menu.
            ContextMenuTarget::Agent { .. } => vec![
                item("Send a line...", Action::SendLine),
                item("Interrupt", Action::Interrupt),
                item("Mark read", Action::MarkRead),
                item("Mark unread", Action::MarkUnread),
                item("Keep", Action::Keep),
                item("Park", Action::Park),
                item("Kill", Action::Kill),
                item("Peek", Action::Peek),
            ],
            // S3: the studio model owns its item list; the row's
            // index is the action.
            ContextMenuTarget::Studio(menu) => menu
                .items()
                .into_iter()
                .enumerate()
                .map(|(index, it)| ContextMenuItem {
                    label: Cow::Owned(it.label),
                    action: ContextMenuAction::Studio(index),
                    dim: it.dim,
                    key: it.key,
                    teal: it.teal,
                })
                .collect(),
        }
    }
}

#[derive(Default)]
pub(crate) struct ContextMenuRender {
    pub rows: Vec<(Rect, usize)>,
}

#[cfg(test)]
pub(crate) fn render_context_menu(
    buffer: &mut Buffer,
    menu: &ContextMenu,
    palette: &Palette,
) -> Option<ContextMenuRender> {
    render_context_menu_in(buffer, menu, buffer.area.right(), palette)
}

/// Render `menu` horizontally clamped inside `screen.x..max_right`.
pub(crate) fn render_context_menu_in(
    buffer: &mut Buffer,
    menu: &ContextMenu,
    max_right: u16,
    palette: &Palette,
) -> Option<ContextMenuRender> {
    let title = menu.title();
    let title_rows = u16::from(title.is_some());
    let items = menu.items();
    let screen = buffer.area;
    let bound_w = max_right.saturating_sub(screen.x).min(screen.width);
    let max_item_width = items
        .iter()
        .map(|item| {
            display_width(&item.label) + item.dim.as_deref().map_or(0, |d| display_width(d) + 3)
        })
        .chain(title.as_deref().map(display_width))
        .max()
        .unwrap_or(0);
    // S3 §1: menus clamp to `min(34, bound_w - 4)`; the reference menus
    // fit under that cap at every tested size, so one rule serves.
    let max_width = bound_w.saturating_sub(4).clamp(1, 34);
    let width = max_item_width
        .saturating_add(4)
        .max(14)
        .min(max_width)
        .min(bound_w.max(1));
    let height = (items.len() as u16)
        .saturating_add(2 + title_rows)
        .min(screen.height.max(1));
    let x = menu
        .x
        .min(screen.x.saturating_add(bound_w.saturating_sub(width)));
    let y = menu.y.min(
        screen
            .y
            .saturating_add(screen.height.saturating_sub(height)),
    );
    let rect = Rect::new(x, y, width, height);
    // S5: a studio menu never keeps the covered rows' modifiers — the
    // panel's fill only patches. The herdr menus keep the patch fill.
    if matches!(menu.target, ContextMenuTarget::Studio(_)) {
        crate::chrome::wide::clear_overlay(buffer, rect);
    }
    let inner = panel(buffer, rect, palette.accent, palette.panel_bg)?;
    if let Some(title) = title.as_deref()
        && inner.height > 0
    {
        let title_style = Style::default()
            .fg(palette.overlay0)
            .bg(palette.panel_bg)
            .add_modifier(Modifier::BOLD);
        put_text(
            buffer,
            inner.x,
            inner.y,
            inner.width,
            &truncate_end(title, inner.width as usize),
            title_style,
        );
    }
    let mut rows = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let row_y = inner
            .y
            .saturating_add(title_rows)
            .saturating_add(index as u16);
        if row_y >= inner.bottom() {
            break;
        }
        let row = Rect::new(inner.x, row_y, inner.width, 1);
        let highlighted = index == menu.highlighted;
        let style = if highlighted {
            Style::default()
                .fg(contrast(palette))
                .bg(palette.accent)
                .add_modifier(Modifier::BOLD)
        } else if item.dim.is_some() {
            Style::default().fg(palette.overlay0).bg(palette.panel_bg)
        } else if item.teal {
            Style::default().fg(palette.teal).bg(palette.panel_bg)
        } else {
            Style::default().fg(palette.text).bg(palette.panel_bg)
        };
        buffer.set_style(row, style);
        let text = match &item.dim {
            Some(reason) => format!("{} · {reason}", item.label),
            None => item.label.to_string(),
        };
        put_text(
            buffer,
            row.x,
            row.y,
            row.width,
            &truncate_end(&text, row.width as usize),
            style,
        );
        rows.push((row, index));
    }
    Some(ContextMenuRender { rows })
}

#[cfg(test)]
#[path = "menu_tests.rs"]
mod tests;

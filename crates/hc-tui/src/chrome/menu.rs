// Derived from herdr src/client/shell/context_menu.rs, state.rs and
// overlays.rs::render_context_menu (Apache-2.0).
//
//! Right-click context menu: the same item sets, ordering, geometry and
//! highlight styles as the reference. `Machine` rows are an hc-tui
//! extension — the reference has no machine menu (parity.md).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::dialogs::{contrast, panel};
use super::palette::Palette;
use super::render::{display_width, put_text};

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
    SendLine,
    Interrupt,
    MarkRead,
    MarkUnread,
    Keep,
    Park,
    Kill,
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
}

#[derive(Debug)]
pub(crate) struct ContextMenu {
    pub target: ContextMenuTarget,
    pub x: u16,
    pub y: u16,
    pub highlighted: usize,
}

pub(crate) struct ContextMenuItem {
    pub label: &'static str,
    pub action: ContextMenuAction,
}

impl ContextMenu {
    pub(crate) fn items(&self) -> Vec<ContextMenuItem> {
        use ContextMenuAction as Action;

        let item = |label, action| ContextMenuItem { label, action };
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
            ],
        }
    }
}

#[derive(Default)]
pub(crate) struct ContextMenuRender {
    pub rows: Vec<(Rect, usize)>,
}

pub(crate) fn render_context_menu(
    buffer: &mut Buffer,
    menu: &ContextMenu,
    palette: &Palette,
) -> Option<ContextMenuRender> {
    let items = menu.items();
    let screen = buffer.area;
    let max_item_width = items
        .iter()
        .map(|item| display_width(item.label))
        .max()
        .unwrap_or(0);
    let width = max_item_width
        .saturating_add(4)
        .max(14)
        .min(screen.width.max(1));
    let height = (items.len() as u16)
        .saturating_add(2)
        .min(screen.height.max(1));
    let x = menu
        .x
        .min(screen.x.saturating_add(screen.width.saturating_sub(width)));
    let y = menu.y.min(
        screen
            .y
            .saturating_add(screen.height.saturating_sub(height)),
    );
    let rect = Rect::new(x, y, width, height);
    let inner = panel(buffer, rect, palette.accent, palette.panel_bg)?;
    let mut rows = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let row_y = inner.y.saturating_add(index as u16);
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
        } else {
            Style::default().fg(palette.text).bg(palette.panel_bg)
        };
        buffer.set_style(row, style);
        put_text(buffer, row.x, row.y, row.width, item.label, style);
        rows.push((row, index));
    }
    Some(ContextMenuRender { rows })
}

#[cfg(test)]
#[path = "menu_tests.rs"]
mod tests;

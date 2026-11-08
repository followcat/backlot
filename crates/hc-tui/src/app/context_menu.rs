// Derived from herdr src/client/shell/context_menu.rs,
// overlay_input.rs and mouse.rs (Apache-2.0).
//
//! Right-click context menu: opens on the hit's target, moves with
//! keys/pointer, and activates the reference's per-target actions.
//! Machine rows are the hc-tui extension documented in parity.md.
use super::input::{MouseButton, MouseEventKind, MouseInput, TerminalKey};
use super::*;
use crate::chrome::menu::{ContextMenu, ContextMenuAction, ContextMenuTarget};
use crate::operations::{Action, Change};
use crossterm::event::KeyCode;
use ratatui::layout::Position;

fn contains(rect: ratatui::layout::Rect, point: (u16, u16)) -> bool {
    rect.contains(Position {
        x: point.0,
        y: point.1,
    })
}

impl App {
    /// `Down(Right)` after pane routing declined the event — the same
    /// dispatch order as the reference: machines, workspace rows of the
    /// active endpoint, tabs, then panes.
    pub(super) fn context_menu_press(&mut self, point: (u16, u16)) {
        if !self.shell.mouse_capture {
            return;
        }
        self.release_pane_mouse();
        if let Some(hit) = self
            .hit_map
            .machines
            .iter()
            .find(|h| contains(h.rect, point))
            .map(|h| h.endpoint_id.clone())
        {
            let name = match &hit {
                crate::chrome::snapshot::ClientEndpointId::Local => None,
                crate::chrome::snapshot::ClientEndpointId::Remote(name) => Some(name.clone()),
            };
            self.menu = Some(ContextMenu {
                target: ContextMenuTarget::Machine { name },
                x: point.0,
                y: point.1,
                highlighted: 0,
            });
            return;
        }
        // Agent rows — including the agent chips drawn inside workspace
        // rows — open the hc-tui extension agent menu (parity.md).
        if let Some(pane_id) = self
            .hit_map
            .agents
            .iter()
            .find(|h| contains(h.0, point))
            .map(|h| h.1.clone())
        {
            self.open_agent_context_menu(&pane_id, point);
            return;
        }
        if !self.shell_state.sidebar_collapsed {
            let workspace_id = self
                .hit_map
                .workspaces
                .iter()
                .find(|h| {
                    h.endpoint_id == self.projection.active_endpoint_id && contains(h.rect, point)
                })
                .map(|h| h.workspace_id.clone());
            if let Some(workspace_id) = workspace_id {
                self.open_workspace_context_menu(&workspace_id, point);
                return;
            }
        }
        if let Some(tab_id) = self
            .hit_map
            .tabs
            .iter()
            .find(|(rect, _)| contains(*rect, point))
            .map(|(_, id)| id.clone())
        {
            self.open_tab_context_menu(&tab_id, point);
            return;
        }
        if let Some(pane_id) = self
            .hit_map
            .panes
            .iter()
            .find(|h| contains(h.rect, point))
            .map(|h| h.pane_id.clone())
        {
            self.open_pane_context_menu(&pane_id, point);
        }
    }

    fn open_workspace_context_menu(&mut self, workspace_id: &str, point: (u16, u16)) {
        let Some(workspace) = self
            .projection
            .endpoints
            .iter()
            .filter_map(|e| e.snapshot.as_deref())
            .flat_map(|s| &s.workspaces)
            .find(|w| w.workspace_id == workspace_id)
        else {
            return;
        };
        // hc-tui's worktree facts are always empty (managed worktree groups
        // are deferred): a git checkout shows the New/Open worktree items,
        // which report as unavailable when activated.
        let is_git = workspace.worktree.is_some() || workspace.branch.is_some();
        self.menu = Some(ContextMenu {
            target: ContextMenuTarget::Workspace {
                workspace_id: workspace_id.to_owned(),
                is_git,
                is_linked_worktree: workspace
                    .worktree
                    .as_ref()
                    .is_some_and(|w| w.is_linked_worktree),
                has_worktree_children: false,
                close_group: false,
                collapsed: false,
            },
            x: point.0,
            y: point.1,
            highlighted: 0,
        });
    }

    fn open_tab_context_menu(&mut self, tab_id: &str, point: (u16, u16)) {
        let Some(tab) = self
            .projection
            .snapshot
            .tabs
            .iter()
            .find(|t| t.tab_id == tab_id)
        else {
            return;
        };
        self.menu = Some(ContextMenu {
            target: ContextMenuTarget::Tab {
                tab_id: tab_id.to_owned(),
                workspace_id: tab.workspace_id.clone(),
            },
            x: point.0,
            y: point.1,
            highlighted: 0,
        });
    }

    fn open_pane_context_menu(&mut self, pane_id: &str, point: (u16, u16)) {
        let Some(pane) = self
            .projection
            .snapshot
            .panes
            .iter()
            .find(|p| p.pane_id == pane_id)
        else {
            return;
        };
        let focused = self.focused.as_deref();
        self.menu = Some(ContextMenu {
            target: ContextMenuTarget::Pane {
                pane_id: pane_id.to_owned(),
                workspace_id: pane.workspace_id.clone(),
                source_pane_id: focused
                    .filter(|focused| *focused != pane_id)
                    .map(str::to_owned),
                has_manual_label: pane.label.is_some(),
                right_click_passthrough: self.right_click_panes.contains(pane_id),
            },
            x: point.0,
            y: point.1,
            highlighted: 0,
        });
    }

    /// Keys while the menu is open — Esc/Up/Down/Enter, like the
    /// reference's `context_menu_key`.
    pub(super) fn menu_key(&mut self, key: &TerminalKey) {
        let combo = crate::input::terminal_key_of(key);
        let Some(menu) = &mut self.menu else {
            return;
        };
        match combo.code {
            KeyCode::Esc => self.menu = None,
            KeyCode::Up => {
                menu.highlighted = menu.highlighted.saturating_sub(1);
            }
            KeyCode::Down => {
                let items = menu.items().len();
                menu.highlighted = (menu.highlighted + 1).min(items.saturating_sub(1));
            }
            KeyCode::Enter => {
                let index = menu.highlighted;
                self.context_menu_activate(index);
            }
            _ => {}
        }
    }

    /// Pointer while the menu is open — the reference highlights on move
    /// and activates on left press; a press outside closes it.
    pub(super) fn menu_mouse(&mut self, mouse: MouseInput) {
        let Some(menu) = &mut self.menu else {
            return;
        };
        match mouse.kind {
            MouseEventKind::Moved | MouseEventKind::Drag(_) => {
                if let Some((_, index)) = self
                    .hit_map
                    .context_menu_rows
                    .iter()
                    .find(|(rect, _)| contains(*rect, (mouse.col, mouse.row)))
                {
                    menu.highlighted = *index;
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let row = self
                    .hit_map
                    .context_menu_rows
                    .iter()
                    .find(|(rect, _)| contains(*rect, (mouse.col, mouse.row)))
                    .map(|(_, index)| *index);
                match row {
                    Some(index) => self.context_menu_activate(index),
                    None => self.menu = None,
                }
            }
            _ => {}
        }
    }

    fn context_menu_activate(&mut self, index: usize) {
        let Some(menu) = self.menu.take() else {
            return;
        };
        let items = menu.items();
        let Some(item) = items.get(index) else {
            return;
        };
        let action = item.action;
        match &menu.target {
            ContextMenuTarget::Workspace { workspace_id, .. } => match action {
                ContextMenuAction::Rename => self.rename_workspace_for(workspace_id),
                ContextMenuAction::Close => self.close_workspace(workspace_id),
                _ => self.show_notice(
                    "Unavailable",
                    "Worktree groups aren't available in hc-tui yet",
                ),
            },
            ContextMenuTarget::Tab {
                tab_id,
                workspace_id,
            } => {
                // The reference focuses the clicked tab before acting.
                self.focus_tab(tab_id);
                match action {
                    ContextMenuAction::NewTab => self.new_tab(),
                    ContextMenuAction::Rename => self.rename_tab(),
                    ContextMenuAction::Close => {
                        self.close_tab_for(workspace_id, tab_id);
                    }
                    _ => {}
                }
            }
            ContextMenuTarget::Pane {
                pane_id,
                workspace_id,
                source_pane_id,
                ..
            } => {
                let tab_id = self
                    .projection
                    .snapshot
                    .panes
                    .iter()
                    .find(|p| &p.pane_id == pane_id)
                    .map(|p| p.tab_id.clone());
                match action {
                    ContextMenuAction::RenamePane => self.rename_pane_for(pane_id),
                    ContextMenuAction::ClearPaneName => {
                        if let Some(term) = self.view.inventory.get(pane_id) {
                            self.submit_action(Action::RenamePane {
                                terminal: pane_id.clone(),
                                created_ms: term.created_ms,
                                name: String::new(),
                            });
                        }
                    }
                    ContextMenuAction::SwapWithFocusedPane => {
                        if let (Some(first), Some(tab_id)) = (source_pane_id, &tab_id) {
                            self.edit_layout_on(
                                workspace_id,
                                tab_id,
                                Change::Swap {
                                    first: first.clone(),
                                    second: pane_id.clone(),
                                },
                            );
                        }
                    }
                    ContextMenuAction::SplitRight | ContextMenuAction::SplitDown => {
                        // `vertical` is the layout direction: down stacks
                        // rows (Vertical), right makes columns (Horizontal).
                        let vertical = action == ContextMenuAction::SplitDown;
                        self.focus_pane(pane_id.clone());
                        self.split_pane(vertical);
                    }
                    ContextMenuAction::Zoom => {
                        if let Some(tab_id) = &tab_id {
                            self.edit_layout_on(
                                workspace_id,
                                tab_id,
                                Change::Zoom(pane_id.clone()),
                            );
                        }
                    }
                    ContextMenuAction::ToggleRightClickPassthrough => {
                        if !self.right_click_panes.remove(pane_id) {
                            self.right_click_panes.insert(pane_id.clone());
                        }
                    }
                    ContextMenuAction::ClosePane => {
                        if let Some(tab_id) = &tab_id {
                            self.close_pane_for(workspace_id, tab_id, pane_id);
                        }
                    }
                    _ => {}
                }
            }
            ContextMenuTarget::Machine { name } => match (name, action) {
                (None, ContextMenuAction::MachineAdd) => self.machine_add(),
                (Some(machine), ContextMenuAction::MachineConnect) => {
                    self.submit_action(Action::Machine(crate::operations::MachineOp::Connect {
                        machine: machine.clone(),
                    }));
                }
                (Some(machine), ContextMenuAction::MachineRename) => {
                    self.machine_rename(machine.clone());
                }
                (Some(machine), ContextMenuAction::MachineRemove) => {
                    self.machine_remove(machine.clone());
                }
                _ => {}
            },
            ContextMenuTarget::Agent { pane_id } => self.agent_menu_action(pane_id, action),
        }
    }
}

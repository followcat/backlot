// Derived from herdr src/client/shell/actions.rs (Apache-2.0).
use super::*;
use crate::layout::NavDirection as D;
use crate::{input::KeybindAction as A, operations::Change};

impl App {
    pub(super) fn layout_action(&mut self, action: A) {
        // With several sidebar endpoints these actions cross machines
        // (the reference's `handle_endpoint_navigation`); the local
        // path below stays the single-endpoint behavior.
        if self.endpoint_navigation(action) {
            self.refresh_projection();
            self.mark_active_seen();
            return;
        }
        let snapshot = &self.projection.snapshot;
        let focus = self.focused.clone();
        let Some(space) = snapshot.focused_workspace_id.as_ref() else {
            return;
        };
        let tabs = snapshot
            .tabs
            .iter()
            .filter(|t| &t.workspace_id == space)
            .map(|t| t.tab_id.clone())
            .collect::<Vec<_>>();
        let tab_index = tabs
            .iter()
            .position(|id| Some(id) == snapshot.focused_tab_id.as_ref())
            .unwrap_or(0);
        let root = snapshot
            .focused_tab_id
            .as_ref()
            .and_then(|id| self.projection.layouts.get(id))
            .and_then(|t| t.root.clone());
        let direction = match action {
            A::FocusPaneLeft | A::SwapPaneLeft | A::ResizePaneLeft => Some(D::Left),
            A::FocusPaneDown | A::SwapPaneDown | A::ResizePaneDown => Some(D::Down),
            A::FocusPaneUp | A::SwapPaneUp | A::ResizePaneUp => Some(D::Up),
            A::FocusPaneRight | A::SwapPaneRight | A::ResizePaneRight => Some(D::Right),
            _ => None,
        };
        let area = self.chrome_layout(self.area.0, self.area.1).pane_surface;
        let neighbour = root.as_ref().and_then(|root| {
            let (layout, ids) = root.layout(focus.as_deref());
            let panes = layout.panes(area);
            let focused = panes.iter().find(|p| p.is_focused)?;
            let id = crate::layout::find_in_direction(focused, direction?, &panes)?;
            ids.get(&id).cloned()
        });
        match action {
            A::SwitchTab(index) => {
                if let Some(tab) = tabs.get(index) {
                    self.focus_tab(tab);
                }
            }
            A::PreviousTab | A::NextTab => {
                if !tabs.is_empty() {
                    let delta = if action == A::PreviousTab { -1 } else { 1 };
                    self.focus_tab(
                        &tabs
                            [(tab_index as isize + delta).rem_euclid(tabs.len() as isize) as usize],
                    );
                }
            }
            A::MoveTabPrevious | A::MoveTabNext => {
                if let Some(tab) = tabs.get(tab_index) {
                    self.edit_layout(Change::MoveTab {
                        tab_id: tab.clone(),
                        direction: if action == A::MoveTabPrevious { -1 } else { 1 },
                        fallback: tabs,
                    });
                }
            }
            A::PreviousWorkspace | A::NextWorkspace | A::SwitchWorkspace(_) => {
                let targets = self
                    .navigate_targets()
                    .into_iter()
                    .filter(|t| t.endpoint_id == self.projection.active_endpoint_id)
                    .collect::<Vec<_>>();
                if targets.is_empty() {
                    return;
                }
                let at = targets
                    .iter()
                    .position(|t| &t.workspace_id == space)
                    .unwrap_or(0);
                let index = match action {
                    A::SwitchWorkspace(i) => i,
                    A::PreviousWorkspace => (at + targets.len() - 1) % targets.len(),
                    _ => (at + 1) % targets.len(),
                };
                if let Some(target) = targets.get(index)
                    && let Some(id) = self.workspace_terminal(target)
                {
                    self.focused = Some(id);
                    self.reveal_workspace(target);
                }
            }
            A::PreviousAgent | A::NextAgent | A::FocusAgent(_) => {
                let ids = crate::chrome::agents::ordered_agent_pane_ids(
                    snapshot,
                    self.shell.agent_panel_sort,
                );
                if ids.is_empty() {
                    return;
                }
                let at = ids.iter().position(|id| Some(id) == focus.as_ref());
                let index = match action {
                    A::FocusAgent(i) => i,
                    A::PreviousAgent => {
                        at.map_or(ids.len() - 1, |at| (at + ids.len() - 1) % ids.len())
                    }
                    _ => at.map_or(0, |at| (at + 1) % ids.len()),
                };
                if let Some(id) = ids.get(index) {
                    self.focused = Some(id.clone());
                    if matches!(action, A::PreviousAgent | A::NextAgent)
                        && !self.hit_map.agents.iter().any(|(_, visible)| visible == id)
                        && !self
                            .hit_map
                            .endpoint_agents
                            .iter()
                            .any(|(_, _, visible)| visible == id)
                    {
                        self.shell_state.agent_scroll = index.min(self.hit_map.agent_max_scroll);
                    }
                }
            }
            A::CyclePaneNext | A::CyclePanePrevious => {
                if let Some(root) = root {
                    let ids = root.terminals();
                    let at = ids
                        .iter()
                        .position(|id| Some(id) == focus.as_ref())
                        .unwrap_or(0);
                    let delta = if action == A::CyclePanePrevious {
                        -1
                    } else {
                        1
                    };
                    self.focused = Some(
                        ids[(at as isize + delta).rem_euclid(ids.len() as isize) as usize].clone(),
                    );
                }
            }
            A::LastPane => {
                if let Some(id) = self
                    .previous_pane
                    .clone()
                    .filter(|id| snapshot.panes.iter().any(|p| &p.pane_id == id))
                {
                    self.focused = Some(id);
                }
            }
            A::FocusPaneLeft | A::FocusPaneDown | A::FocusPaneUp | A::FocusPaneRight => {
                if neighbour.is_some() {
                    self.focused = neighbour;
                }
            }
            A::SwapPaneLeft | A::SwapPaneDown | A::SwapPaneUp | A::SwapPaneRight => {
                if let (Some(first), Some(second)) = (focus, neighbour) {
                    self.edit_layout(Change::Swap { first, second });
                }
            }
            A::ResizePaneLeft | A::ResizePaneDown | A::ResizePaneUp | A::ResizePaneRight => {
                if let (Some(terminal), Some(direction)) = (focus, direction) {
                    self.edit_layout(Change::Resize {
                        terminal,
                        direction,
                        area,
                        delta: 0.05,
                    });
                }
            }
            A::Zoom => {
                if let Some(id) = focus {
                    self.edit_layout(Change::Zoom(id));
                }
            }
            A::EnterResizeMode => self.mode = Mode::Resize,
            A::NewTab => self.new_tab(),
            A::SplitVertical | A::SplitHorizontal => self.split_pane(action == A::SplitHorizontal),
            A::RenameTab => self.rename_tab(),
            A::RenamePane => self.rename_pane(),
            A::ClosePane | A::CloseTab => self.close_layout(action == A::CloseTab),
            _ => {}
        }
        self.refresh_projection();
        self.mark_active_seen();
    }
}

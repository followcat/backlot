// Derived from herdr src/client/shell/mouse.rs (Apache-2.0).
use super::*;

impl App {
    /// `LeftDrag`: continue the grab; one cell promotes a tab press.
    pub(in crate::app) fn mouse_drag(&mut self, point: (u16, u16)) {
        if let Some(press) = &self.tab_press
            && (self.tab_dragged
                || point.0.abs_diff(press.start_column) >= 1
                || point.1.abs_diff(press.start_row) >= 1)
        {
            self.tab_drag_index = self.tab_drop_index(point);
            self.tab_dragged |= self.tab_drag_index.is_some();
        }
        match self.chrome_drag.clone() {
            Some(ChromeDrag::PaneSplit { .. }) => self.drag_split(point, false),
            Some(ChromeDrag::SidebarWidth) => self.set_sidebar_width_from_column(point.0),
            Some(ChromeDrag::SidebarSection) => self.set_sidebar_section_from_row(point.1),
            Some(ChromeDrag::WorkspaceScrollbar { grab_row_offset }) => {
                if let Some(metrics) = self.hit_map.workspace_scroll_metrics {
                    let offset = scroll::scrollbar_offset_from_drag_row(
                        metrics,
                        self.hit_map.workspace_scrollbar,
                        point.1,
                        grab_row_offset,
                    );
                    let next = metrics.max_offset_from_bottom.saturating_sub(offset);
                    self.shell_state.workspace_scroll = next;
                }
            }
            Some(ChromeDrag::AgentScrollbar { grab_row_offset }) => {
                if let Some(metrics) = self.hit_map.agent_scroll_metrics {
                    let offset = scroll::scrollbar_offset_from_drag_row(
                        metrics,
                        self.hit_map.agent_scrollbar,
                        point.1,
                        grab_row_offset,
                    );
                    let next = metrics.max_offset_from_bottom.saturating_sub(offset);
                    self.shell_state.agent_scroll = next;
                }
            }
            Some(ChromeDrag::NavigatorScrollbar { grab_row_offset }) => {
                if let Some(metrics) = self.navigator_hits.metrics {
                    let offset = scroll::scrollbar_offset_from_drag_row(
                        metrics,
                        self.navigator_hits.scrollbar,
                        point.1,
                        grab_row_offset,
                    );
                    self.scroll_navigator_to(
                        metrics.max_offset_from_bottom.saturating_sub(offset),
                        metrics.viewport_rows,
                    );
                }
            }
            Some(ChromeDrag::PaneScrollbar {
                pane_id,
                grab_row_offset,
            }) => {
                if let Some(hit) = self
                    .hit_map
                    .panes
                    .iter()
                    .find(|h| h.pane_id == pane_id && h.scrollbar_rect.is_some())
                {
                    let offset = pane_scrollbar_offset(hit, point.1, grab_row_offset);
                    if let Some(offset) = offset {
                        self.pane_scroll_to_offset(&pane_id, offset);
                    }
                }
            }
            None => {}
        }
    }

    /// `LeftUp`: a press that never became a drag is a click —
    /// workspace/tab presses focus their target.
    pub(in crate::app) fn mouse_release(&mut self, point: (u16, u16)) {
        if matches!(self.chrome_drag, Some(ChromeDrag::PaneSplit { .. })) {
            self.drag_split(point, true);
        }
        self.chrome_drag = None;
        if let Some(press) = self.workspace_press.take() {
            let moved = point.0.abs_diff(press.start_column) > DRAG_THRESHOLD
                || point.1.abs_diff(press.start_row) > DRAG_THRESHOLD;
            if !moved {
                self.focus_workspace(&press.endpoint_id, &press.workspace_id);
            }
        }
        if let Some(press) = self.tab_press.take() {
            let promoted = std::mem::take(&mut self.tab_dragged);
            self.tab_drag_index = None;
            if !promoted {
                self.focus_tab(&press.tab_id);
            } else if let Some(insert) = self.tab_drop_index(point) {
                let tabs = self
                    .projection
                    .snapshot
                    .tabs
                    .iter()
                    .filter(|t| {
                        Some(&t.workspace_id)
                            == self.projection.snapshot.focused_workspace_id.as_ref()
                    })
                    .map(|t| t.tab_id.clone())
                    .collect::<Vec<_>>();
                if let Some(source) = tabs.iter().position(|id| id == &press.tab_id) {
                    let destination = insert
                        .saturating_sub(usize::from(source < insert))
                        .min(tabs.len().saturating_sub(1));
                    self.edit_layout(crate::operations::Change::MoveTab {
                        tab_id: press.tab_id,
                        direction: destination as isize - source as isize,
                        fallback: tabs,
                    });
                }
            }
        }
    }

    /// `set_sidebar_width_from_column`: `col+1` clamped to the
    /// validated sidebar bounds.
    pub(in crate::app) fn set_sidebar_width_from_column(&mut self, column: u16) {
        let (min, max) = crate::config::validated_sidebar_bounds(
            self.shell.sidebar_min_width,
            self.shell.sidebar_max_width,
        )
        .unwrap_or((18, 36));
        self.sidebar_width = column.saturating_add(1).clamp(min, max);
        self.sidebar_width_manual = true;
    }

    /// `set_sidebar_section_from_row`: split ratio over the divider
    /// rect's height, clamped `0.1..=0.9`.
    pub(in crate::app) fn set_sidebar_section_from_row(&mut self, row: u16) {
        let divider = self.hit_map.sidebar_divider;
        if divider.height == 0 {
            return;
        }
        let ratio = (row.saturating_sub(divider.y) as f32 / divider.height as f32).clamp(0.1, 0.9);
        self.shell_state.sidebar_section_split = ratio;
    }

    /// Focus helpers — a "focus" is the single attach: the terminal
    /// behind the hit becomes `focused`, the workspace's active tab
    /// is marked seen, and the pane re-syncs.
    pub(in crate::app) fn focus_pane(&mut self, pane_id: String) {
        self.focused = Some(pane_id);
        self.refresh_projection();
        self.mark_active_seen();
        self.refresh_projection();
        self.sync_target();
    }

    /// Workspace row click: its active tab's first pane.
    pub(in crate::app) fn focus_workspace(
        &mut self,
        _endpoint_id: &crate::chrome::snapshot::ClientEndpointId,
        workspace_id: &str,
    ) {
        let target = crate::chrome::navigate::WorkspaceNavigationTarget {
            endpoint_id: _endpoint_id.clone(),
            workspace_id: workspace_id.to_owned(),
            boot_id: String::new(),
            generation: None,
        };
        if let Some(id) = self.workspace_terminal(&target) {
            self.focus_pane(id);
        }
    }

    /// Tab row click: its first pane.
    pub(in crate::app) fn focus_tab(&mut self, tab_id: &str) {
        let target = self
            .projection
            .layouts
            .get(tab_id)
            .and_then(|tab| tab.focused.clone())
            .or_else(|| {
                self.projection
                    .snapshot
                    .panes
                    .iter()
                    .find(|p| p.tab_id == tab_id)
                    .map(|p| p.pane_id.clone())
            });
        if let Some(pane_id) = target {
            self.focus_pane(pane_id);
        }
    }

    /// Tab-band wheel: cycle the focused workspace's tabs by `dir`,
    /// wrapping — the reference's adjacent-tab scroll.
    pub(in crate::app) fn focus_adjacent_tab(&mut self, dir: isize) {
        let snapshot = &self.projection.snapshot;
        let Some(workspace) = snapshot
            .workspaces
            .iter()
            .find(|w| Some(&w.workspace_id) == snapshot.focused_workspace_id.as_ref())
        else {
            return;
        };
        let tabs: Vec<String> = snapshot
            .tabs
            .iter()
            .filter(|t| t.workspace_id == workspace.workspace_id)
            .map(|t| t.tab_id.clone())
            .collect();
        if tabs.is_empty() {
            return;
        }
        let current = tabs
            .iter()
            .position(|t| *t == workspace.active_tab_id)
            .unwrap_or(0);
        let len = tabs.len() as isize;
        let next = ((current as isize + dir).rem_euclid(len)) as usize;
        let tab_id = tabs[next].clone();
        self.focus_tab(&tab_id);
    }

    /// Pane scrollbar drag/click → the VT viewport offset.
    pub(in crate::app) fn pane_scroll_to_offset(&mut self, pane_id: &str, offset: usize) {
        if let Some(vt) = self.displayed_pane_mut(pane_id).and_then(|p| p.vt())
            && let Some(state) = vt.scrollbar()
        {
            let maximum = state.total.saturating_sub(state.len) as usize;
            vt.scroll_to_row(maximum.saturating_sub(offset));
        }
    }

    /// Agent-panel sort toggle: flips the runtime sort between the
    /// two reference orders.
    pub(in crate::app) fn flip_agent_sort(&mut self) {
        self.agent_panel_sort_manual = true;
        use crate::config::AgentPanelSortConfig as S;
        self.shell.agent_panel_sort = match self.shell.agent_panel_sort {
            S::Spaces => S::Priority,
            S::Priority => S::Spaces,
        };
    }

    /// Sidebar group collapse toggle — local groups land on
    /// `collapsed_groups`, remote on the per-endpoint map, matching
    /// the render state's lookup.
    pub(in crate::app) fn toggle_collapsed_group(
        &mut self,
        endpoint_id: &crate::chrome::snapshot::ClientEndpointId,
        key: String,
    ) {
        use crate::chrome::snapshot::ClientEndpointId;
        match endpoint_id {
            ClientEndpointId::Local => {
                if !self.shell_state.collapsed_groups.remove(&key) {
                    self.shell_state.collapsed_groups.insert(key);
                }
            }
            ClientEndpointId::Remote(_) => {
                let groups = self
                    .shell_state
                    .remote_collapsed_groups
                    .entry(endpoint_id.clone())
                    .or_default();
                if !groups.remove(&key) {
                    groups.insert(key);
                }
            }
        }
    }
}

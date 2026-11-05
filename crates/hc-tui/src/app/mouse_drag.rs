// Derived from herdr src/client/shell/mouse.rs (Apache-2.0).
use super::*;

impl App {
    /// `LeftDrag`: continue the grab. Workspace/tab presses only record
    /// intent in M2 — reordering is deferred, so no drag promotion.
    pub(in crate::app) fn mouse_drag(&mut self, point: (u16, u16)) {
        match self.chrome_drag {
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
            Some(ChromeDrag::PaneScrollbar { grab_row_offset }) => {
                if let Some(hit) = self
                    .hit_map
                    .panes
                    .iter()
                    .find(|h| h.scrollbar_rect.is_some())
                {
                    let offset = pane_scrollbar_offset(hit, point.1, grab_row_offset);
                    if let Some(offset) = offset {
                        self.pane_scroll_to_offset(offset);
                    }
                }
            }
            None => {}
        }
    }

    /// `LeftUp`: a press that never became a drag is a click —
    /// workspace/tab presses focus their target.
    pub(in crate::app) fn mouse_release(&mut self, point: (u16, u16)) {
        self.chrome_drag = None;
        if let Some(press) = self.workspace_press.take() {
            let moved = point.0.abs_diff(press.start_column) > DRAG_THRESHOLD
                || point.1.abs_diff(press.start_row) > DRAG_THRESHOLD;
            if !moved {
                self.focus_workspace(&press.endpoint_id, &press.workspace_id);
            }
        }
        if let Some(press) = self.tab_press.take() {
            let moved = point.0.abs_diff(press.start_column) > DRAG_THRESHOLD
                || point.1.abs_diff(press.start_row) > DRAG_THRESHOLD;
            if !moved {
                self.focus_tab(&press.tab_id);
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
        let snapshot = &self.projection.snapshot;
        let Some(workspace) = snapshot
            .workspaces
            .iter()
            .find(|w| w.workspace_id == workspace_id)
        else {
            return;
        };
        let active = workspace.active_tab_id.clone();
        let target = snapshot
            .panes
            .iter()
            .find(|p| p.workspace_id == workspace_id && p.tab_id == active)
            .or_else(|| {
                snapshot
                    .panes
                    .iter()
                    .find(|p| p.workspace_id == workspace_id)
            })
            .map(|p| p.pane_id.clone());
        if let Some(pane_id) = target {
            self.focus_pane(pane_id);
        }
    }

    /// Tab row click: its first pane.
    pub(in crate::app) fn focus_tab(&mut self, tab_id: &str) {
        let target = self
            .projection
            .snapshot
            .panes
            .iter()
            .find(|p| p.tab_id == tab_id)
            .map(|p| p.pane_id.clone());
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
    pub(in crate::app) fn pane_scroll_to_offset(&mut self, offset: usize) {
        if let Some(vt) = self.pane.vt()
            && let Some(state) = vt.scrollbar()
        {
            let maximum = state.total.saturating_sub(state.len) as usize;
            vt.scroll_to_row(maximum.saturating_sub(offset));
        }
    }

    /// Agent-panel sort toggle: flips the runtime sort between the
    /// two reference orders.
    pub(in crate::app) fn flip_agent_sort(&mut self) {
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

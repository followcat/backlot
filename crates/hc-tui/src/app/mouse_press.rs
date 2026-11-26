// Derived from herdr src/client/shell/mouse.rs (Apache-2.0).
use super::*;

impl App {
    /// `LeftDown` in the reference's hit order. `previous_pane_click`
    /// carries the double-click candidate the prologue took.
    pub(in crate::app) fn mouse_press(
        &mut self,
        mouse: &MouseInput,
        previous_pane_click: Option<crate::app::pane_selection::PaneClick>,
    ) {
        let point = (mouse.col, mouse.row);
        let hits = self.hit_map.clone();
        self.chrome_drag = None;
        self.workspace_press = None;
        self.tab_press = None;
        self.tab_drag_index = None;
        self.tab_dragged = false;
        // Mobile: the header's switch chip opens the Navigate switcher.
        if self.mobile() && contains(hits.mobile_switch, point) {
            self.enter_workspace_picker();
            return;
        }
        if contains(hits.sidebar_toggle, point) {
            self.shell_state.sidebar_collapsed = !self.shell_state.sidebar_collapsed;
            self.panel.sidebar_collapsed = self.shell_state.sidebar_collapsed;
            self.sidebar_collapsed_manual = true;
            self.persist_chrome_preferences();
            return;
        }
        if contains(hits.sidebar_divider, point) {
            let now = Instant::now();
            let double = self
                .last_divider_click
                .is_some_and(|t| now.duration_since(t) <= DOUBLE_CLICK);
            self.last_divider_click = Some(now);
            if double {
                // U1: the reset lands on the mode's configured width —
                // backlot's `[sidebar] width` in studio mode.
                self.sidebar_width = self.configured_sidebar_width();
                self.sidebar_width_manual = false;
                self.persist_chrome_preferences();
            } else {
                self.chrome_drag = Some(ChromeDrag::SidebarWidth);
                self.set_sidebar_width_from_column(point.0);
            }
            return;
        }
        if contains(hits.sidebar_section_divider, point) {
            self.chrome_drag = Some(ChromeDrag::SidebarSection);
            self.set_sidebar_section_from_row(point.1);
            return;
        }
        if contains(hits.workspace_scrollbar, point) {
            if let Some(metrics) = hits.workspace_scroll_metrics {
                match scroll::scrollbar_thumb_grab_offset(
                    metrics,
                    hits.workspace_scrollbar,
                    point.1,
                ) {
                    Some(grab) => {
                        self.chrome_drag = Some(ChromeDrag::WorkspaceScrollbar {
                            grab_row_offset: grab,
                        });
                    }
                    None => {
                        let offset = scroll::scrollbar_offset_from_row(
                            metrics,
                            hits.workspace_scrollbar,
                            point.1,
                        );
                        let next = metrics.max_offset_from_bottom.saturating_sub(offset);
                        self.shell_state.workspace_scroll = next;
                    }
                }
            }
            return;
        }
        if contains(hits.agent_scrollbar, point) {
            if let Some(metrics) = hits.agent_scroll_metrics {
                match scroll::scrollbar_thumb_grab_offset(metrics, hits.agent_scrollbar, point.1) {
                    Some(grab) => {
                        self.chrome_drag = Some(ChromeDrag::AgentScrollbar {
                            grab_row_offset: grab,
                        });
                    }
                    None => {
                        let offset = scroll::scrollbar_offset_from_row(
                            metrics,
                            hits.agent_scrollbar,
                            point.1,
                        );
                        let next = metrics.max_offset_from_bottom.saturating_sub(offset);
                        // U1: the cast section's scrollbar, same slot.
                        if self.studio.active {
                            self.studio.cast_scroll = next;
                        } else {
                            self.shell_state.agent_scroll = next;
                        }
                    }
                }
            }
            return;
        }
        if contains(hits.agent_sort_toggle, point) {
            self.flip_agent_sort();
            self.shell_state.agent_scroll = 0;
            return;
        }
        // U1: the cast's `others` row expands herdr's agents in place.
        if self.studio.active && contains(hits.cast_others, point) {
            self.studio.others_expanded = !self.studio.others_expanded;
            return;
        }
        // The sidebar's `menu` button opens `Ctrl+B ?` (S5 §2) —
        // studio mode only; in `--plain` herdr's dead hit stays dead.
        if !self.plain && contains(hits.global_launcher, point) {
            self.help = true;
            self.help_state = crate::chrome::help::HelpState::default();
            return;
        }
        // W1: a studio-mode machines-tree row that is not a space — a
        // `stage down` studio starts the stage, the `finished (n)` group
        // folds, a finished studio opens its menu.
        if let Some(hit) = hits
            .studio_rows
            .iter()
            .find(|h| contains(h.rect, point))
            .cloned()
        {
            self.studio_row_activated(&hit, point);
            return;
        }
        // Machine rows: collapse toggle (or the active endpoint's row)
        // toggles the endpoint section; other online rows would activate
        // the endpoint — hc-tui is single-endpoint, so the toggle case
        // is the reachable one.
        if let Some(hit) = hits.machines.iter().find(|h| contains(h.rect, point)) {
            let id = hit.endpoint_id.clone();
            if (contains(hit.collapse_toggle, point)
                || contains(hit.status_badge, point)
                || id == self.projection.active_endpoint_id)
                && !self.shell_state.collapsed_endpoints.remove(&id)
            {
                self.shell_state.collapsed_endpoints.insert(id);
            }
            return;
        }
        // The lot pill owns its reserved segment before the tab bar's
        // own hits — it opens the Hollywood bar like `alt+y`.
        if contains(hits.lot_pill, point) {
            self.hollywood_toggle();
            return;
        }
        // The studio header's name focuses the dock and sets `StudioFocus`
        // on that studio.
        if contains(hits.studio_header, point)
            && let Some(key) = hits.studio_key.clone()
        {
            self.studio.set_studio_focus(key.clone());
            if self.panel_mode_at(self.area.0) == crate::panel::PanelMode::Closed {
                self.panel.mode = crate::panel::PanelMode::Expanded;
            }
            self.panel.focused = true;
            self.panel.selected = Some(crate::panel::Target::Studio(key));
            self.panel.selected_row = None;
            self.panel_reveal_selected();
            return;
        }
        if contains(hits.tab_scroll_left, point) {
            self.shell_state.tab_scroll = self.shell_state.tab_scroll.saturating_sub(1);
            return;
        }
        if contains(hits.tab_scroll_right, point) {
            self.shell_state.tab_scroll = self.shell_state.tab_scroll.saturating_add(1);
            return;
        }
        if contains(hits.new_tab, point) {
            self.new_tab();
            return;
        }
        if self.update_toast_hit.width > 0 && contains(self.update_toast_hit, point) {
            self.notice = None;
            self.open_update();
            return;
        }
        // The badge's update hint opens the update card; the version
        // itself stays inert — the hit only covers the hint.
        if (self.update_ready_version().is_some() || self.restart_ready.is_some())
            && contains(hits.update_badge, point)
        {
            self.open_update();
            return;
        }
        // The sidebar's `new` row. The classic one-machine footer keeps
        // `new_workspace` (herdr in `--plain`, where the spaces sidebar
        // still draws); the machine-first `new · <machine>` footer is
        // the New Studio dialog's click path, with that machine
        // preselected (U6b) — the same condition `render_shell` routes
        // its footer on.
        if contains(hits.new_workspace, point) {
            if self.federated_sidebar() {
                let machine = match &self.projection.active_endpoint_id {
                    crate::chrome::snapshot::ClientEndpointId::Remote(name) => Some(name.clone()),
                    _ => None,
                };
                self.new_studio_open(machine);
            } else {
                self.new_workspace();
            }
            return;
        }
        let group_toggle = hits.workspaces.iter().find_map(|hit| {
            let (rect, key) = hit.group_toggle.as_ref()?;
            contains(*rect, point).then(|| (hit.endpoint_id.clone(), key.clone()))
        });
        if let Some((endpoint_id, key)) = group_toggle {
            self.toggle_collapsed_group(&endpoint_id, key);
            return;
        }
        if let Some(hit) = hits.workspaces.iter().find(|h| contains(h.rect, point)) {
            self.workspace_press = Some(WorkspacePress {
                endpoint_id: hit.endpoint_id.clone(),
                workspace_id: hit.workspace_id.clone(),
                start_column: point.0,
                start_row: point.1,
            });
            return;
        }
        if let Some((_, tab_id)) = hits.tabs.iter().find(|(rect, _)| contains(*rect, point))
            && self.shell.mouse_capture
        {
            self.tab_press = Some(TabPress {
                tab_id: tab_id.clone(),
                start_column: point.0,
                start_row: point.1,
            });
            return;
        }
        if let Some((_, endpoint_id, pane_id)) = hits
            .endpoint_agents
            .iter()
            .find(|(rect, _, _)| contains(*rect, point))
        {
            let _ = endpoint_id;
            self.focus_pane(pane_id.clone());
            return;
        }
        if let Some((_, pane_id)) = hits.agents.iter().find(|(rect, _)| contains(*rect, point)) {
            self.focus_pane(pane_id.clone());
            return;
        }
        if let Some(hit) = hits.panes.iter().find(|h| contains(h.rect, point)).cloned() {
            if let (Some(track), Some(metrics)) = (hit.scrollbar_rect, hit.scroll)
                && contains(track, point)
            {
                let grab = scroll::scrollbar_thumb_grab_offset(metrics, track, point.1);
                self.chrome_drag = Some(ChromeDrag::PaneScrollbar {
                    pane_id: hit.pane_id.clone(),
                    grab_row_offset: grab,
                });
                if grab.is_none() {
                    let offset = scroll::scrollbar_offset_from_row(metrics, track, point.1);
                    self.pane_scroll_to_offset(&hit.pane_id, offset);
                }
                return;
            }
            // Only the inner rect anchors a selection — a press on the
            // border still focuses but selects nothing (the reference's
            // `contains(inner_rect)` gate). Reporting panes never reach
            // here; `route_pane_mouse` forwarded them already.
            if contains(hit.inner_rect, point) {
                self.press_pane_selection(&hit, mouse, previous_pane_click);
            }
            self.focus_pane(hit.pane_id.clone());
        }
    }
}

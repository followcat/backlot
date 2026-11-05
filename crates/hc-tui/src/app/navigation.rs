// Derived from herdr src/client/shell/input.rs and composition.rs (Apache-2.0).
use super::*;

impl App {
    /// One resolved keybinding action. Read-only hc-tui executes the
    /// navigation/selection subset; the rest resolve faithfully but are
    /// inert until their milestone (documented in docs/parity.md).
    pub(super) fn on_action(&mut self, action: crate::input::KeybindAction, _width: u16) {
        use crate::input::KeybindAction as A;
        match action {
            A::Detach => self.quitting = true,
            A::Help => {
                self.help = true;
                self.help_state = crate::chrome::help::HelpState::default();
            }
            A::ToggleSidebar => {
                self.shell_state.sidebar_collapsed = !self.shell_state.sidebar_collapsed;
            }
            A::WorkspacePicker => self.enter_workspace_picker(),
            _ => {}
        }
        self.sync_target();
    }

    /// Enter Navigate with the pending highlight on the focused
    /// workspace — `focused_navigation_target`. Shared by the keybind
    /// and the mobile switch button (the reference's one entry path).
    pub(super) fn enter_workspace_picker(&mut self) {
        self.before_navigate = self.focused.clone();
        self.mode = Mode::Navigate;
        self.prefix.reset();
        self.shell_state.mobile_switcher_scroll = 0;
        self.shell_state.reveal_mobile_workspace = false;
        // Refresh first so a focus set without a publish still
        // resolves (finding 12).
        self.refresh_projection();
        self.navigate_target = crate::chrome::navigate::focused_navigation_target(
            &self.projection.endpoints,
            &self.projection.active_endpoint_id,
        );
    }

    /// The workspace-navigation targets across all online endpoints —
    /// the sidebar-visible rows honoring collapse state.
    pub(super) fn navigate_targets(&self) -> Vec<WorkspaceNavigationTarget> {
        let mobile = self.area.0 <= self.shell.mobile_width_threshold;
        crate::chrome::navigate::workspace_navigation_targets(
            &self.projection.endpoints,
            self.shell_state.sidebar_collapsed,
            mobile,
            true,
            &self.shell_state.collapsed_groups,
            &self.shell_state.remote_collapsed_groups,
        )
    }

    /// The terminal a workspace target focuses — its active tab's first
    /// pane, else its first pane (`pane_id` is the route address).
    pub(super) fn workspace_terminal(&self, target: &WorkspaceNavigationTarget) -> Option<String> {
        let endpoint = self
            .projection
            .endpoints
            .iter()
            .find(|e| e.endpoint_id == target.endpoint_id)?;
        let snap = endpoint.snapshot.as_deref()?;
        let ws = snap
            .workspaces
            .iter()
            .find(|w| w.workspace_id == target.workspace_id)?;
        snap.panes
            .iter()
            .find(|p| p.workspace_id == ws.workspace_id && p.tab_id == ws.active_tab_id)
            .or_else(|| {
                snap.panes
                    .iter()
                    .find(|p| p.workspace_id == ws.workspace_id)
            })
            .map(|p| p.pane_id.clone())
    }

    /// `state.rs::reveal_workspace` — expand the highlighted
    /// workspace's collapsed group and position its sidebar entry
    /// on-screen (the reference's `workspace_scroll = pos.min(max)`).
    pub(super) fn reveal_workspace(&mut self, target: &WorkspaceNavigationTarget) {
        self.shell_state
            .collapsed_endpoints
            .remove(&target.endpoint_id);
        if target.endpoint_id.is_local() {
            self.shell_state
                .collapsed_groups
                .remove(&target.workspace_id);
        } else if let Some(groups) = self
            .shell_state
            .remote_collapsed_groups
            .get_mut(&target.endpoint_id)
        {
            groups.remove(&target.workspace_id);
        }
        // The reference scrolls directly only on the single-endpoint
        // desktop path; multi-endpoint/mobile reveal through the flag.
        let mobile = self.area.0 <= self.shell.mobile_width_threshold;
        if self.projection.endpoints.len() != 1 || mobile {
            return;
        }
        // Already on-screen: the hit map covers it, no scroll change.
        if self
            .hit_map
            .workspaces
            .iter()
            .any(|hit| hit.workspace_id == target.workspace_id)
        {
            return;
        }
        let position = self
            .projection
            .endpoints
            .iter()
            .find(|e| e.endpoint_id == target.endpoint_id)
            .and_then(|e| e.snapshot.as_deref())
            .and_then(|snapshot| {
                let groups = if target.endpoint_id.is_local() {
                    &self.shell_state.collapsed_groups
                } else {
                    self.shell_state
                        .remote_collapsed_groups
                        .get(&target.endpoint_id)
                        .unwrap_or(&self.shell_state.collapsed_groups)
                };
                crate::chrome::sidebar_render::workspace_entries(snapshot, groups)
                    .iter()
                    .position(|entry| {
                        snapshot.workspaces[entry.index].workspace_id == target.workspace_id
                    })
            });
        if let Some(position) = position {
            self.shell_state.workspace_scroll = position.min(self.hit_map.workspace_max_scroll);
        }
    }

    /// One Navigate outcome against the highlight.
    pub(super) fn on_navigate(&mut self, nav: input::Navigate) {
        use input::Navigate::*;
        let mobile = self.area.0 <= self.shell.mobile_width_threshold;
        match nav {
            Move(delta) => {
                let mut targets = self.navigate_targets();
                if let Some(index) = crate::chrome::navigate::move_index(
                    &targets,
                    self.navigate_target.as_ref(),
                    delta as isize,
                    mobile,
                ) {
                    let target = targets.swap_remove(index);
                    self.reveal_workspace(&target);
                    self.navigate_target = Some(target);
                    self.shell_state.reveal_navigation_workspace = true;
                    // The mobile switcher scrolls to the highlight too.
                    self.shell_state.reveal_mobile_workspace = mobile;
                }
            }
            // `1..9` selects the nth *active-endpoint* target and
            // commits like Enter — the reference's digit binding is
            // `SwitchWorkspace(index)` against the active snapshot's
            // navigation entries, exiting Navigate in one press.
            Select(index) => {
                let targets: Vec<_> = self
                    .navigate_targets()
                    .into_iter()
                    .filter(|t| t.endpoint_id == self.projection.active_endpoint_id)
                    .collect();
                if let Some(target) = targets.get(index).cloned() {
                    self.reveal_workspace(&target);
                    if crate::chrome::navigate::navigation_target_valid(
                        &self.projection.endpoints,
                        &target,
                    ) && let Some(terminal) = self.workspace_terminal(&target)
                    {
                        self.focused = Some(terminal);
                        self.refresh_projection();
                        self.mark_active_seen();
                        self.refresh_projection();
                    }
                    self.navigate_target = None;
                    self.mode = Mode::Normal;
                }
            }
            Accept => {
                if self.navigate_target.as_ref().is_none_or(|target| {
                    !crate::chrome::navigate::navigation_target_valid(
                        &self.projection.endpoints,
                        target,
                    )
                }) {
                    self.show_notice(
                        "Endpoint unavailable",
                        "Workspace is no longer available; select a connected workspace",
                    );
                    return;
                }
                if let Some(target) = self.navigate_target.clone()
                    && crate::chrome::navigate::navigation_target_valid(
                        &self.projection.endpoints,
                        &target,
                    )
                    && let Some(terminal) = self.workspace_terminal(&target)
                {
                    self.focused = Some(terminal);
                    self.refresh_projection();
                    self.mark_active_seen();
                    self.refresh_projection();
                }
                self.navigate_target = None;
                self.mode = Mode::Normal;
            }
            Cancel => {
                self.navigate_target = None;
                self.focused = self.before_navigate.clone();
                self.mode = Mode::Normal;
            }
        }
        self.sync_target();
    }

    /// A mobile-switcher row press — the reference's
    /// `ClientMobileTarget` dispatch. Read-only subset: Machine picks
    /// the endpoint's first pane; Workspace/Tab pick their panes;
    /// Agent picks its pane; New* are M3 and inert. A successful focus
    /// exits Navigate back to Terminal.
    pub(super) fn on_mobile_target(&mut self, target: &crate::chrome::render::ClientMobileTarget) {
        use crate::chrome::render::ClientMobileTarget as T;
        let pane = match target {
            T::Machine(endpoint_id) => self
                .projection
                .endpoints
                .iter()
                .find(|e| &e.endpoint_id == endpoint_id)
                .and_then(|e| e.snapshot.as_deref())
                .and_then(|s| s.panes.first())
                .map(|p| p.pane_id.clone()),
            T::Workspace {
                endpoint_id,
                workspace_id,
            } => {
                let nav = WorkspaceNavigationTarget {
                    endpoint_id: endpoint_id.clone(),
                    workspace_id: workspace_id.clone(),
                    boot_id: String::new(),
                    generation: None,
                };
                self.workspace_terminal(&nav)
            }
            T::Tab {
                endpoint_id,
                tab_id,
            } => self
                .projection
                .endpoints
                .iter()
                .find(|e| &e.endpoint_id == endpoint_id)
                .and_then(|e| e.snapshot.as_deref())
                .and_then(|s| s.panes.iter().find(|p| &p.tab_id == tab_id))
                .map(|p| p.pane_id.clone()),
            T::Agent { pane_id, .. } => Some(pane_id.clone()),
            T::NewWorkspace | T::NewTab => None,
        };
        if let Some(pane_id) = pane {
            self.focused = Some(pane_id);
            self.refresh_projection();
            self.mark_active_seen();
            self.refresh_projection();
            self.navigate_target = None;
            self.mode = Mode::Normal;
            self.sync_target();
        }
    }
}

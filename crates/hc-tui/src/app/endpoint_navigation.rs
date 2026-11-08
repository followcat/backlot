// Derived from herdr src/client/shell/endpoint_navigation.rs (Apache-2.0).

use std::collections::{HashMap, HashSet};

use super::*;
use crate::chrome::endpoints::AggregateAgentTarget;
use crate::chrome::navigate::WorkspaceNavigationTarget;
use crate::input::KeybindAction as A;

impl App {
    /// `handle_endpoint_navigation`: with more than one sidebar
    /// endpoint, workspace and agent actions cycle every Online
    /// machine in sidebar order. Returns true when the action was
    /// consumed (including an empty-targets no-op); the caller keeps
    /// the local single-endpoint path otherwise.
    pub(super) fn endpoint_navigation(&mut self, action: A) -> bool {
        if self.projection.endpoints.len() <= 1 {
            return false;
        }
        match action {
            A::PreviousWorkspace | A::NextWorkspace => {
                self.navigate_endpoint_workspace(action);
                true
            }
            A::PreviousAgent | A::NextAgent | A::FocusAgent(_) => {
                self.navigate_endpoint_agent(action);
                true
            }
            _ => false,
        }
    }

    /// One flat list of `(endpoint, workspace)` over every Online
    /// endpoint in sidebar order, ignoring collapsed endpoints and
    /// groups — the reference passes an empty collapse set.
    fn endpoint_workspace_targets(&self) -> Vec<WorkspaceNavigationTarget> {
        crate::chrome::navigate::workspace_navigation_targets(
            &self.projection.endpoints,
            false,
            false,
            false,
            &HashSet::new(),
            &HashMap::new(),
        )
    }

    fn navigate_endpoint_workspace(&mut self, action: A) {
        let targets = self.endpoint_workspace_targets();
        if targets.is_empty() {
            return;
        }
        let focused_workspace_id = self.projection.snapshot.focused_workspace_id.clone();
        let active_endpoint_id = self.projection.active_endpoint_id.clone();
        let current = targets.iter().position(|target| {
            target.endpoint_id == active_endpoint_id
                && Some(&target.workspace_id) == focused_workspace_id.as_ref()
        });
        let next = match (current, action) {
            (Some(index), A::PreviousWorkspace) => (index + targets.len() - 1) % targets.len(),
            (Some(index), A::NextWorkspace) => (index + 1) % targets.len(),
            (None, A::PreviousWorkspace) => targets.len() - 1,
            _ => 0,
        };
        let Some(target) = targets.get(next).cloned() else {
            return;
        };
        // Focusing the machine-qualified pane id is what switches the
        // active endpoint (it is derived from the focused address).
        if let Some(terminal) = self.workspace_terminal(&target) {
            self.focused = Some(terminal);
            self.refresh_projection();
        }
        self.reveal_workspace(&target);
        self.shell_state.reveal_focused_workspace = true;
    }

    fn navigate_endpoint_agent(&mut self, action: A) {
        let agents = crate::chrome::endpoints::online_agent_targets(
            &self.projection.endpoints,
            &self.projection.active_endpoint_id,
            self.shell.agent_panel_sort,
        );
        if agents.is_empty() {
            return;
        }
        let next = match action {
            A::FocusAgent(index) => {
                if index >= agents.len() {
                    return;
                }
                index
            }
            A::PreviousAgent | A::NextAgent => {
                let focused_pane_id = self.projection.snapshot.focused_pane_id.clone();
                let active_endpoint_id = self.projection.active_endpoint_id.clone();
                let current = agents.iter().position(|target| {
                    target.endpoint_id == active_endpoint_id
                        && Some(&target.pane_id) == focused_pane_id.as_ref()
                });
                match (current, action) {
                    (Some(index), A::PreviousAgent) => (index + agents.len() - 1) % agents.len(),
                    (Some(index), A::NextAgent) => (index + 1) % agents.len(),
                    (None, A::PreviousAgent) => agents.len() - 1,
                    _ => 0,
                }
            }
            _ => return,
        };
        let Some(target) = agents.get(next).cloned() else {
            return;
        };
        self.focus_pane(target.pane_id.clone());
        self.reveal_endpoint_agent(&target);
    }

    /// `reveal_endpoint_agent`: expand the target's endpoint and the
    /// focused workspace's collapsed group, flag the workspace reveal,
    /// and scroll the agent list — unless the row was already visible
    /// on the last frame (`agent_scroll` stays untouched then).
    /// `row_index` is the row's position in the full aggregate list,
    /// the index the sidebar renders.
    fn reveal_endpoint_agent(&mut self, target: &AggregateAgentTarget) {
        if let Some(workspace_id) = self.projection.snapshot.focused_workspace_id.clone() {
            self.reveal_workspace(&WorkspaceNavigationTarget {
                endpoint_id: target.endpoint_id.clone(),
                workspace_id,
                boot_id: String::new(),
                generation: None,
            });
        }
        self.shell_state.reveal_focused_workspace = true;
        let visible = self
            .hit_map
            .agents
            .iter()
            .any(|(_, pane_id)| pane_id == &target.pane_id)
            || self
                .hit_map
                .endpoint_agents
                .iter()
                .any(|(_, _, pane_id)| pane_id == &target.pane_id);
        if !visible {
            self.shell_state.agent_scroll = target.row_index.min(self.hit_map.agent_max_scroll);
        }
    }
}

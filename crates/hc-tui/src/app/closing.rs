// Derived from herdr src/client/shell/overlay_input.rs and workspace.rs (Apache-2.0).
use super::dialogs::{Body, Target};
use super::*;
use crate::{chrome::dialogs::Confirm, operations::Action};
impl App {
    /// Build the shared `Action::Close`: identity + incarnation fences, a
    /// foreign-membership check per pane, and a deterministic fallback focus.
    /// `tab` scopes to one tab of the workspace; `None` is the whole space.
    fn close_action(
        &self,
        workspace_id: &str,
        tab: Option<&str>,
        ids: Vec<String>,
    ) -> Option<Action> {
        if ids.is_empty() {
            return None;
        }
        let snapshot = &self.projection.snapshot;
        let machine = ids
            .first()
            .and_then(|id| id.split_once('/').map(|(m, _)| m.to_owned()));
        let terminals = ids
            .iter()
            .filter_map(|id| {
                self.view
                    .inventory
                    .get(id)
                    .map(|t| (id.clone(), t.created_ms))
            })
            .collect();
        let active_machine = match &self.projection.active_endpoint_id {
            crate::chrome::snapshot::ClientEndpointId::Local => None,
            crate::chrome::snapshot::ClientEndpointId::Remote(name) => Some(name.clone()),
        };
        let fallback = if machine == active_machine {
            self.projection
                .layouts
                .iter()
                .map(|(id, t)| {
                    (
                        crate::project::layout_key(machine.as_deref(), id),
                        t.clone(),
                    )
                })
                .collect()
        } else {
            // Other endpoints do not publish roots here; hide() still prunes.
            Default::default()
        };
        let memberships = snapshot
            .panes
            .iter()
            .filter(|p| ids.contains(&p.pane_id))
            .map(|p| {
                (
                    p.pane_id.clone(),
                    (p.workspace_id.clone(), p.tab_id.clone()),
                )
            })
            .collect();
        let survivors = snapshot
            .panes
            .iter()
            .filter(|p| p.workspace_id == workspace_id)
            .filter(|p| tab.is_none_or(|t| p.tab_id == t))
            .filter(|p| !ids.contains(&p.pane_id))
            .map(|p| p.pane_id.clone())
            .collect::<Vec<_>>();
        let next = if !survivors.is_empty() {
            self.previous_pane
                .clone()
                .filter(|id| survivors.contains(id))
                .or_else(|| survivors.first().cloned())
        } else if let Some(tab) = tab {
            let tabs = snapshot
                .tabs
                .iter()
                .filter(|t| t.workspace_id == workspace_id)
                .collect::<Vec<_>>();
            let at = tabs.iter().position(|t| t.tab_id == tab).unwrap_or(0);
            let remaining = tabs.iter().filter(|t| t.tab_id != tab).collect::<Vec<_>>();
            remaining
                .get(at.saturating_sub(1).min(remaining.len().saturating_sub(1)))
                .and_then(|t| {
                    self.projection
                        .layouts
                        .get(&t.tab_id)
                        .and_then(|layout| layout.focused.clone())
                        .or_else(|| {
                            snapshot
                                .panes
                                .iter()
                                .find(|p| p.tab_id == t.tab_id)
                                .map(|p| p.pane_id.clone())
                        })
                })
                .or_else(|| {
                    snapshot
                        .panes
                        .iter()
                        .find(|p| !ids.contains(&p.pane_id))
                        .map(|p| p.pane_id.clone())
                })
        } else {
            snapshot
                .panes
                .iter()
                .find(|p| !ids.contains(&p.pane_id))
                .map(|p| p.pane_id.clone())
        };
        Some(Action::Close {
            terminals,
            memberships,
            fallback,
            next,
        })
    }

    fn close_confirm(&mut self, action: Action, workspace_id: &str) {
        let Some(space) = self
            .projection
            .endpoints
            .iter()
            .filter_map(|e| e.snapshot.as_deref())
            .flat_map(|s| &s.workspaces)
            .find(|s| s.workspace_id == workspace_id)
        else {
            return;
        };
        let count = self
            .projection
            .endpoints
            .iter()
            .filter_map(|e| e.snapshot.as_deref())
            .flat_map(|s| &s.panes)
            .filter(|p| p.workspace_id == workspace_id)
            .count();
        let detail = format!(
            "{} — {} {}",
            space.label,
            count,
            if count == 1 { "pane" } else { "panes" }
        );
        self.open_dialog(
            Target::Close(action),
            Body::Confirm(Confirm {
                title: "Close workspace?".into(),
                detail,
            }),
        );
    }

    /// Close the focused pane, or its whole tab — the `close_pane`/`close_tab`
    /// bindings and their context-menu equivalents on the focused target.
    pub(super) fn close_layout(&mut self, whole_tab: bool) {
        let (workspace, all_count, last_tab, action) = {
            let snapshot = &self.projection.snapshot;
            let Some(tab) = snapshot.focused_tab_id.clone() else {
                return;
            };
            let Some(workspace) = snapshot.focused_workspace_id.clone() else {
                return;
            };
            let all = snapshot
                .panes
                .iter()
                .filter(|p| p.tab_id == tab)
                .collect::<Vec<_>>();
            let ids = all
                .iter()
                .filter(|p| whole_tab || Some(&p.pane_id) == self.focused.as_ref())
                .map(|p| p.pane_id.clone())
                .collect::<Vec<_>>();
            let last_tab = !snapshot
                .tabs
                .iter()
                .any(|t| t.workspace_id == workspace && t.tab_id != tab);
            let Some(action) = self.close_action(&workspace, Some(&tab), ids) else {
                return;
            };
            (workspace, all.len(), last_tab, action)
        };
        if self.shell.confirm_close && last_tab && (whole_tab || all_count == 1) {
            self.close_confirm(action, &workspace);
        } else {
            self.submit_action(action);
        }
    }

    /// A context-menu close may target a tab or pane that is not focused;
    /// the reference `request_tab_close`/`request_pane_close` take the id.
    pub(super) fn close_tab_for(&mut self, workspace_id: &str, tab_id: &str) {
        let (ids, last_tab) = {
            let snapshot = &self.projection.snapshot;
            let ids = snapshot
                .panes
                .iter()
                .filter(|p| p.workspace_id == workspace_id && p.tab_id == tab_id)
                .map(|p| p.pane_id.clone())
                .collect::<Vec<_>>();
            let last_tab = !snapshot
                .tabs
                .iter()
                .any(|t| t.workspace_id == workspace_id && t.tab_id != tab_id);
            (ids, last_tab)
        };
        let Some(action) = self.close_action(workspace_id, Some(tab_id), ids) else {
            return;
        };
        if self.shell.confirm_close && last_tab {
            self.close_confirm(action, workspace_id);
        } else {
            self.submit_action(action);
        }
    }

    pub(super) fn close_pane_for(&mut self, workspace_id: &str, tab_id: &str, pane_id: &str) {
        let (panes, last_tab) = {
            let snapshot = &self.projection.snapshot;
            let panes = snapshot
                .panes
                .iter()
                .filter(|p| p.workspace_id == workspace_id && p.tab_id == tab_id)
                .count();
            let last_tab = !snapshot
                .tabs
                .iter()
                .any(|t| t.workspace_id == workspace_id && t.tab_id != tab_id);
            (panes, last_tab)
        };
        let Some(action) = self.close_action(workspace_id, Some(tab_id), vec![pane_id.to_owned()])
        else {
            return;
        };
        if self.shell.confirm_close && last_tab && panes == 1 {
            self.close_confirm(action, workspace_id);
        } else {
            self.submit_action(action);
        }
    }

    /// `request_workspace_close`: kill every pane of the workspace (close
    /// group is identical for a single root), with the same confirmation.
    pub(super) fn close_workspace(&mut self, workspace_id: &str) {
        let ids = self
            .projection
            .endpoints
            .iter()
            .filter_map(|e| e.snapshot.as_deref())
            .flat_map(|s| &s.panes)
            .filter(|p| p.workspace_id == workspace_id)
            .map(|p| p.pane_id.clone())
            .collect::<Vec<_>>();
        let Some(action) = self.close_action(workspace_id, None, ids) else {
            return;
        };
        if self.shell.confirm_close {
            self.close_confirm(action, workspace_id);
        } else {
            self.submit_action(action);
        }
    }

    pub(super) fn close_membership_valid(&self, action: &Action) -> bool {
        let Action::Close { memberships, .. } = action else {
            return false;
        };
        memberships.iter().all(|(id, (space, tab))| {
            self.projection
                .endpoints
                .iter()
                .filter_map(|e| e.snapshot.as_deref())
                .flat_map(|s| &s.panes)
                .any(|p| &p.pane_id == id && &p.workspace_id == space && &p.tab_id == tab)
        })
    }
}

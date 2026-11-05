// Derived from herdr src/client/shell/overlay_input.rs and workspace.rs (Apache-2.0).
use super::dialogs::{Body, Target};
use super::*;
use crate::{chrome::dialogs::Confirm, operations::Action};
impl App {
    pub(super) fn close_layout(&mut self, whole_tab: bool) {
        let snapshot = &self.projection.snapshot;
        let Some(tab) = snapshot.focused_tab_id.as_ref() else {
            return;
        };
        let all = snapshot
            .panes
            .iter()
            .filter(|p| &p.tab_id == tab)
            .collect::<Vec<_>>();
        let ids = all
            .iter()
            .filter(|p| whole_tab || Some(&p.pane_id) == self.focused.as_ref())
            .map(|p| p.pane_id.clone())
            .collect::<Vec<_>>();
        let terminals = ids
            .iter()
            .filter_map(|id| {
                self.view
                    .inventory
                    .get(id)
                    .map(|t| (id.clone(), t.created_ms))
            })
            .collect();
        let machine = self
            .focused
            .as_deref()
            .and_then(|id| id.split_once('/').map(|(m, _)| m));
        let fallback = self
            .projection
            .layouts
            .iter()
            .map(|(id, t)| (crate::project::layout_key(machine, id), t.clone()))
            .collect();
        let survivors = all
            .iter()
            .filter(|p| !ids.contains(&p.pane_id))
            .map(|p| p.pane_id.clone())
            .collect::<Vec<_>>();
        let next = if !survivors.is_empty() {
            self.previous_pane
                .clone()
                .filter(|id| survivors.contains(id))
                .or_else(|| survivors.first().cloned())
        } else {
            let tabs = snapshot
                .tabs
                .iter()
                .filter(|t| Some(&t.workspace_id) == snapshot.focused_workspace_id.as_ref())
                .collect::<Vec<_>>();
            let at = tabs.iter().position(|t| &t.tab_id == tab).unwrap_or(0);
            let remaining = tabs.iter().filter(|t| &t.tab_id != tab).collect::<Vec<_>>();
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
        };
        let memberships = all
            .iter()
            .filter(|p| ids.contains(&p.pane_id))
            .map(|p| {
                (
                    p.pane_id.clone(),
                    (p.workspace_id.clone(), p.tab_id.clone()),
                )
            })
            .collect();
        let action = Action::Close {
            terminals,
            memberships,
            fallback,
            next,
        };
        let last_tab = !snapshot.tabs.iter().any(|t| {
            Some(&t.workspace_id) == snapshot.focused_workspace_id.as_ref() && &t.tab_id != tab
        });
        if self.shell.confirm_close && last_tab && (whole_tab || all.len() == 1) {
            let Some(space) = snapshot
                .workspaces
                .iter()
                .find(|s| Some(&s.workspace_id) == snapshot.focused_workspace_id.as_ref())
            else {
                return;
            };
            let count = snapshot
                .panes
                .iter()
                .filter(|p| p.workspace_id == space.workspace_id)
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

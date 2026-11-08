//! Own-document writes and confirmed-version fencing, never foreign writes.
use super::*;
use crate::operations::{Action, Change, Edit, Outcome, Request, Store};
use hc_proto::watch::{DocKey, Document, Link, OpenRpc};

pub(super) fn key() -> DocKey {
    DocKey {
        machine: None,
        name: "hc-tui".into(),
    }
}

impl App {
    pub fn set_operations(&mut self, open: OpenRpc) {
        self.start_seen_worker(open.clone());
        self.operations = Some(Store::new(open));
    }

    pub(super) fn submit_action(&mut self, action: Action) {
        if !matches!(self.view.link, Link::Online) || !matches!(self.view.rpc, Link::Online) {
            self.show_notice(
                "Layout unavailable",
                "A live inventory subscription is required",
            );
            return;
        }
        let result = self
            .operations
            .as_ref()
            .ok_or_else(|| "Layout actions are unavailable".to_owned())
            .and_then(|store| {
                store.submit(Request {
                    core: self
                        .view
                        .core
                        .as_ref()
                        .map(|c| c.id.clone())
                        .unwrap_or_default(),
                    action,
                })
            });
        match result {
            Ok(()) => self.pending_actions += 1,
            Err(error) => self.show_notice("Action unavailable", &error),
        }
    }

    pub(super) async fn operation_result(
        store: &mut Option<Store>,
    ) -> Option<Result<Outcome, String>> {
        match store {
            Some(store) => store.results.recv().await,
            None => std::future::pending().await,
        }
    }

    pub(super) fn on_operation(&mut self, result: Option<Result<Outcome, String>>) {
        self.pending_actions = self.pending_actions.saturating_sub(1);
        match result {
            Some(Ok(Outcome::Ran(terminal))) => self.on_popup_spawned(terminal),
            Some(Ok(Outcome::Done)) => {}
            Some(Ok(Outcome::Written(written)))
                if self
                    .view
                    .core
                    .as_ref()
                    .is_some_and(|c| c.id == written.core) =>
            {
                let current = self
                    .view
                    .documents
                    .get(&key())
                    .map(|d| d.version)
                    .unwrap_or(0);
                if written.version >= current {
                    let stored = Document {
                        value: serde_json::to_value(written.document).expect("finite layout"),
                        version: written.version,
                        ..Default::default()
                    };
                    self.layout_confirmed = Some((written.core, stored.clone()));
                    self.view.documents.insert(key(), stored);
                }
                if let Some(terminal) = written.created {
                    let observed = self.view.inventory.get(&terminal);
                    self.command_overlay = Some(popup::CommandOverlay {
                        terminal,
                        created_ms: observed.map(|t| t.created_ms),
                        seen: observed.is_some(),
                    });
                }
            }
            Some(Err(error)) => self.show_notice("Action failed", &error),
            None => {
                self.operations = None;
                self.pending_actions = 0;
                self.show_notice("Action failed", "The action worker stopped");
            }
            _ => {}
        }
        self.reconcile_published_view();
    }

    pub(super) fn retain_confirmed_layout(&mut self) {
        if let Some((core, stored)) = &self.layout_confirmed {
            if self.view.core.as_ref().is_none_or(|c| &c.id != core) {
                self.layout_confirmed = None;
                self.pending_actions = 0;
            } else if self
                .view
                .documents
                .get(&key())
                .is_none_or(|d| d.version < stored.version)
            {
                self.view.documents.insert(key(), stored.clone());
            } else {
                self.layout_confirmed = None;
            }
        }
    }

    pub(super) fn layout_edit(&self, change: Change) -> Option<Edit> {
        let snapshot = &self.projection.snapshot;
        let tab = snapshot.focused_tab_id.as_ref()?;
        let space = snapshot.focused_workspace_id.as_ref()?;
        let machine = self
            .focused
            .as_deref()
            .and_then(|s| s.split_once('/').map(|(m, _)| m));
        Some(Edit {
            tab: crate::project::layout_key(machine, tab),
            space: crate::project::layout_key(machine, space),
            fallback: self.projection.layouts.get(tab)?.root.clone()?,
            incarnations: self
                .view
                .inventory
                .terminals()
                .iter()
                .map(|(id, t)| (id.clone(), t.created_ms))
                .collect(),
            change,
        })
    }

    pub(super) fn edit_layout(&mut self, change: Change) {
        if let Some(edit) = self.layout_edit(change) {
            self.submit_action(Action::Edit(edit));
        }
    }

    /// The machine that owns a workspace's panes, from whichever endpoint
    /// publishes them. `None` means the local machine.
    pub(super) fn workspace_machine(&self, workspace_id: &str) -> Option<String> {
        let by_pane = self
            .projection
            .endpoints
            .iter()
            .filter_map(|e| e.snapshot.as_deref())
            .flat_map(|s| &s.panes)
            .find(|p| p.workspace_id == workspace_id)
            .and_then(|p| p.pane_id.split_once('/').map(|(m, _)| m.to_owned()));
        by_pane.or_else(|| {
            self.projection
                .endpoints
                .iter()
                .filter_map(|e| e.snapshot.as_deref().map(|s| (&e.endpoint_id, s)))
                .find(|(_, s)| s.workspaces.iter().any(|w| w.workspace_id == workspace_id))
                .and_then(|(id, _)| match id {
                    crate::chrome::snapshot::ClientEndpointId::Remote(name) => Some(name.clone()),
                    crate::chrome::snapshot::ClientEndpointId::Local => None,
                })
        })
    }

    /// A layout edit on an explicit tab of an explicit workspace — the
    /// context-menu path, which may act without focusing the target first.
    /// Only the active endpoint publishes layout roots, so edits on remote
    /// workspaces degrade to a no-op rather than guessing machine keys.
    pub(super) fn layout_edit_on(
        &self,
        workspace_id: &str,
        tab_id: &str,
        change: Change,
    ) -> Option<Edit> {
        let machine = self.workspace_machine(workspace_id);
        Some(Edit {
            tab: crate::project::layout_key(machine.as_deref(), tab_id),
            space: crate::project::layout_key(machine.as_deref(), workspace_id),
            fallback: self.projection.layouts.get(tab_id)?.root.clone()?,
            incarnations: self
                .view
                .inventory
                .terminals()
                .iter()
                .map(|(id, t)| (id.clone(), t.created_ms))
                .collect(),
            change,
        })
    }

    pub(super) fn edit_layout_on(&mut self, workspace_id: &str, tab_id: &str, change: Change) {
        if let Some(edit) = self.layout_edit_on(workspace_id, tab_id, change) {
            self.submit_action(Action::Edit(edit));
        }
    }

    pub(super) fn persist_focus(&mut self) {
        if self.operations.is_some()
            && let Some(focus) = self.focused.clone()
        {
            self.edit_layout(Change::Focus(focus));
        }
    }
}

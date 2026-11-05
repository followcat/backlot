//! Own-document writes and confirmed-version fencing, never foreign writes.
use super::*;
use crate::operations::{Action, Change, Edit, Request, Store, Written};
use hc_proto::watch::{DocKey, Document, Link, OpenRpc};

pub(super) fn key() -> DocKey {
    DocKey {
        machine: None,
        name: "hc-tui".into(),
    }
}

impl App {
    pub fn set_operations(&mut self, open: OpenRpc) {
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
    ) -> Option<Result<Option<Written>, String>> {
        match store {
            Some(store) => store.results.recv().await,
            None => std::future::pending().await,
        }
    }

    pub(super) fn on_operation(&mut self, result: Option<Result<Option<Written>, String>>) {
        self.pending_actions = self.pending_actions.saturating_sub(1);
        match result {
            Some(Ok(Some(written)))
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

    pub(super) fn persist_focus(&mut self) {
        if self.operations.is_some()
            && let Some(focus) = self.focused.clone()
        {
            self.edit_layout(Change::Focus(focus));
        }
    }
}

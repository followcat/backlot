//! Persistent observers retain appearance. A pane that has been visible also
//! holds a silent size lease for as long as its observer stays cached.
//! The focused pane keeps
//! its existing original-wire input leases, including across focus changes.
use super::*;

/// The current tab is pinned; retain at most this many off-tab VTs. Every
/// cached VT wire is observe-only. Scrollback remains bounded per Pane.
pub(super) const RECENT_PANES: usize = 32;

impl App {
    pub(super) async fn shutdown_panes(&mut self) {
        let mut closing = tokio::task::JoinSet::new();
        for mut pane in std::mem::take(&mut self.observers)
            .into_values()
            .chain(std::mem::take(&mut self.size_leases).into_values())
        {
            closing.spawn(async move {
                pane.shutdown().await;
                pane.close_failure().map(str::to_owned)
            });
        }
        if let Some(mut popup) = self.popup.take() {
            closing.spawn(async move {
                popup.pane.shutdown().await;
                popup.pane.close_failure().map(str::to_owned)
            });
        }
        if let Some(mut peek) = self.peek.take() {
            closing.spawn(async move {
                peek.pane.shutdown().await;
                peek.pane.close_failure().map(str::to_owned)
            });
        }
        self.pane.shutdown().await;
        while let Some(result) = closing.join_next().await {
            if let Ok(Some(error)) = result {
                self.background_failure.get_or_insert(error);
            }
        }
    }

    pub(super) fn reset_displayed_scroll(&mut self) {
        let Some(id) = self.focused.clone() else {
            return;
        };
        if let Some(vt) = self.displayed_pane_mut(&id).and_then(|p| p.vt())
            && let Some(scroll) = vt.scrollbar()
        {
            vt.scroll_to_row(scroll.total.saturating_sub(scroll.len) as usize);
        }
    }
    pub(super) fn follow_shared_focus(&mut self) {
        if self.pending_actions > 0 {
            return;
        }
        let key = hc_proto::watch::DocKey {
            machine: None,
            name: "hc-tui".into(),
        };
        let Some(stored) = self.view.documents.get(&key).filter(|d| d.error.is_none()) else {
            return;
        };
        let stamp = (
            self.view
                .core
                .as_ref()
                .map(|c| c.id.clone())
                .unwrap_or_default(),
            stored.version,
        );
        if self.layout_seen.as_ref() == Some(&stamp) {
            return;
        }
        if let Ok(document) = crate::structure::Document::decode(stored.value.clone()) {
            if let Some(focus) = document.focus {
                if self.view.inventory.get(&focus).is_none() {
                    return;
                }
                self.focused = Some(focus);
            }
            self.layout_seen = Some(stamp);
        }
    }

    pub(super) fn visible_panes(&self) -> Vec<String> {
        let Some(tab) = self
            .projection
            .snapshot
            .focused_tab_id
            .as_ref()
            .and_then(|id| self.projection.layouts.get(id))
        else {
            return self.focused.iter().cloned().collect();
        };
        if let Some(zoomed) = &tab.zoomed {
            return vec![zoomed.clone()];
        }
        tab.root.as_ref().map(|r| r.terminals()).unwrap_or_default()
    }

    pub(super) fn sync_observers(&mut self) {
        let current = self
            .projection
            .snapshot
            .focused_tab_id
            .as_ref()
            .and_then(|id| self.projection.layouts.get(id))
            .and_then(|t| t.root.as_ref())
            .map(|root| root.terminals())
            .unwrap_or_else(|| self.focused.iter().cloned().collect());
        self.observer_mru
            .retain(|id| self.view.inventory.get(id).is_some());
        for id in &current {
            self.observer_mru.retain(|old| old != id);
            self.observer_mru.push_back(id.clone());
        }
        let keep = self
            .observer_mru
            .iter()
            .rev()
            .filter(|id| !current.contains(id))
            .take(RECENT_PANES)
            .cloned()
            .chain(current.iter().cloned())
            .collect::<std::collections::HashSet<_>>();
        self.observer_mru.retain(|id| keep.contains(id));
        self.observers.retain(|id, _| keep.contains(id));
        for id in keep {
            let pane = self.observers.entry(id.clone()).or_insert_with(|| {
                let mut pane = pane::Pane::new(self.pane_tx.clone(), self.open_attach.clone());
                pane.set_host_facts(self.host_facts.clone());
                pane.want(Some(Spec {
                    terminal: id,
                    role: Role::Observe,
                }));
                pane
            });
            pane.on_view(&self.view);
        }
    }

    pub(super) fn pane_event(&mut self, event: PaneEv, now: Instant) {
        let route = event.route();
        if self.pane.owns_route(route) {
            self.pane.on_event(event, now);
        } else if self
            .popup
            .as_mut()
            .is_some_and(|p| p.pane.owns_route(route))
            && let Some(popup) = self.popup.as_mut()
        {
            popup.pane.on_event(event, now);
        } else if self.peek.as_mut().is_some_and(|p| p.pane.owns_route(route))
            && let Some(peek) = self.peek.as_mut()
        {
            peek.pane.on_event(event, now);
        } else if let Some(pane) = self
            .observers
            .values_mut()
            .chain(self.size_leases.values_mut())
            .find(|p| p.owns_route(route))
        {
            pane.on_event(event, now);
        }
    }

    pub(super) fn pane_wake(&self) -> Option<Instant> {
        self.pane
            .next_wake()
            .into_iter()
            .chain(self.popup.iter().filter_map(|p| p.pane.next_wake()))
            .chain(self.peek.iter().filter_map(|p| p.pane.next_wake()))
            .chain(
                self.observers
                    .values()
                    .chain(self.size_leases.values())
                    .filter_map(|p| p.next_wake()),
            )
            .min()
    }

    pub(super) fn pane_retry(&mut self, now: Instant) {
        self.pane.fire_retry(now);
        if let Some(popup) = self.popup.as_mut() {
            popup.pane.fire_retry(now);
        }
        if let Some(peek) = self.peek.as_mut() {
            peek.pane.fire_retry(now);
        }
        for pane in self
            .observers
            .values_mut()
            .chain(self.size_leases.values_mut())
        {
            pane.fire_retry(now);
        }
    }

    /// The pane whose screen to draw: whichever has a complete paint. While
    /// neither has one, a retained observer screen beats an attach with
    /// nothing to show, so a switch or resize never flashes an empty pane.
    pub(super) fn displayed_pane(&self, id: &str) -> Option<&pane::Pane> {
        let observer = self.observers.get(id);
        let active = (self.pane.target() == Some(id)).then_some(&self.pane);
        observer
            .filter(|p| p.has_fresh_paint())
            .or_else(|| active.filter(|p| p.has_fresh_paint()))
            .or_else(|| observer.filter(|p| p.vt_ref().is_some()))
            .or(active)
            .or(observer)
    }

    pub(super) fn displayed_pane_mut(&mut self, id: &str) -> Option<&mut pane::Pane> {
        if self
            .displayed_pane(id)
            .is_some_and(|p| std::ptr::eq(p, &self.pane))
        {
            Some(&mut self.pane)
        } else {
            self.observers.get_mut(id)
        }
    }
}

//! Persistent observers retain appearance. A pane that has been visible also
//! holds a silent size lease for as long as its observer stays cached.
//! The focused pane keeps
//! its existing original-wire input leases, including across focus changes.
use super::*;

/// The current tab is pinned; retain at most this many off-tab VTs. Every
/// cached VT wire is observe-only. Scrollback remains bounded per Pane.
pub(super) const RECENT_PANES: usize = 32;

/// An event routed to a pane the frame can show. A `RouteClosed`
/// without error retires a drained wire — bookkeeping only — and a
/// `WriterProgress` retry is invisible unless the input backlog badge
/// or the pane notice moved. Everything else can repaint.
fn visible_pane_event(pane: &mut pane::Pane, event: PaneEv, now: Instant) -> bool {
    match &event {
        PaneEv::RouteClosed { error: None, .. } => {
            pane.on_event(event, now);
            false
        }
        PaneEv::WriterProgress { .. } => {
            let before = (pane.input_backlog(), pane.notice().is_some());
            pane.on_event(event, now);
            (pane.input_backlog(), pane.notice().is_some()) != before
        }
        _ => {
            pane.on_event(event, now);
            true
        }
    }
}

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

    /// A pane-bound page key pages the *displayed* pane's scrollback —
    /// the reference applies its PageKey rule to whatever the pane
    /// shows, so the scroll must land on the same surface
    /// `reset_displayed_scroll` and wheel scrolling use: a
    /// fresh-painted observer can hold the screen during reattach.
    /// `true` once scrolled: the key is UI-consumed and never forwards.
    pub(super) fn pane_page_scroll(&mut self, key: &input::TerminalKey) -> bool {
        let Some(id) = self.pane.target().map(str::to_owned) else {
            return false;
        };
        self.displayed_pane_mut(&id)
            .is_some_and(|pane| pane.page_scroll(key))
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
        if let Ok(document) = crate::structure::Document::decode_ref(&stored.value) {
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
        // The focused terminal paints through its own active attach — a
        // duplicate observer would feed a second VT for nothing. One
        // opened before the focus moved stays only until the active
        // wire finishes its first paint, so a switch never flashes a
        // blank pane; a new one for the focused terminal never opens.
        let focused_ready = self.pane.has_fresh_paint();
        let active_target = self.pane.target();
        self.observers.retain(|id, _| {
            keep.contains(id) && (active_target != Some(id.as_str()) || !focused_ready)
        });
        for id in keep {
            if active_target == Some(id.as_str()) && !self.observers.contains_key(&id) {
                continue;
            }
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
        // Off-tab observers pace their DATA onto the shared flush grid;
        // visible ones are rendered surfaces and stay live.
        let visible = self.visible_panes();
        for (id, pane) in &mut self.observers {
            pane.set_off_tab(!visible.iter().any(|v| v == id));
        }
    }

    /// Route an attach-actor report to its owning pane; `true` only
    /// when the event can change what the frame paints. Off-tab
    /// observers and size leases update retained state silently —
    /// their bells and clipboard writes surface through the drains.
    pub(super) fn pane_event(&mut self, event: PaneEv, now: Instant) -> bool {
        let route = event.route();
        if self.pane.owns_route(route) {
            let dirty = visible_pane_event(&mut self.pane, event, now);
            // The focused wire's fresh paint retires the stale observer
            // kept warm for the switch — close it now instead of
            // waiting for the next view publish.
            if self.pane.has_fresh_paint()
                && let Some(id) = self.pane.target()
            {
                self.observers.remove(id);
            }
            return dirty;
        }
        if let Some(popup) = self
            .popup
            .as_mut()
            .filter(|popup| popup.pane.owns_route(route))
        {
            return visible_pane_event(&mut popup.pane, event, now);
        }
        if let Some(peek) = self
            .peek
            .as_mut()
            .filter(|peek| peek.pane.owns_route(route))
        {
            return visible_pane_event(&mut peek.pane, event, now);
        }
        // Size leases never paint; check them before pricing out the
        // visible set an observer event might need.
        if let Some(pane) = self
            .size_leases
            .values_mut()
            .find(|pane| pane.owns_route(route))
        {
            pane.on_event(event, now);
            return false;
        }
        let visible = self.visible_panes();
        if let Some((id, pane)) = self
            .observers
            .iter_mut()
            .find(|(_, pane)| pane.owns_route(route))
        {
            if visible.iter().any(|pane_id| pane_id == id) {
                return visible_pane_event(pane, event, now);
            }
            pane.on_event(event, now);
        }
        false
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

    /// `true` when a due retry moved an on-screen pane — the new
    /// attach state paints next.
    pub(super) fn pane_retry(&mut self, now: Instant) -> bool {
        let mut dirty = self.pane.fire_retry(now);
        if let Some(popup) = self.popup.as_mut() {
            dirty |= popup.pane.fire_retry(now);
        }
        if let Some(peek) = self.peek.as_mut() {
            dirty |= peek.pane.fire_retry(now);
        }
        let visible = self.visible_panes();
        for (id, pane) in self.observers.iter_mut() {
            if visible.iter().any(|pane_id| pane_id == id) {
                dirty |= pane.fire_retry(now);
            } else {
                pane.fire_retry(now);
            }
        }
        for pane in self.size_leases.values_mut() {
            pane.fire_retry(now);
        }
        dirty
    }

    /// The pane whose screen to draw: whichever has a complete paint —
    /// the focused wire wins a tie since its stream is the one input
    /// sees. While neither has one, a retained observer screen beats an
    /// attach with nothing to show, so a switch or resize never
    /// flashes an empty pane.
    pub(super) fn displayed_pane(&self, id: &str) -> Option<&pane::Pane> {
        let observer = self.observers.get(id);
        let active = (self.pane.target() == Some(id)).then_some(&self.pane);
        active
            .filter(|p| p.has_fresh_paint())
            .or_else(|| observer.filter(|p| p.has_fresh_paint()))
            .or_else(|| observer.filter(|p| p.vt_ref().is_some()))
            .or_else(|| active.filter(|p| p.vt_ref().is_some()))
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

#[cfg(test)]
#[path = "multipane_tests.rs"]
mod tests;

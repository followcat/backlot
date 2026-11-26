//! View synchronization: a watcher publish lands in `self.view`, then
//! the projection, focus fallbacks and pane targets reconcile against
//! it. Split from `mod.rs` at the publish/reproject seam.

use super::*;

/// The `reveal_focused_tab` trigger: the focused tab changed, or the
/// tab bar's id order did — compared lazily by id, without a single
/// `String` clone or `Vec` allocation.
fn tab_focus_or_order_changed(
    old: &crate::chrome::snapshot::ClientShellSnapshot,
    new: &crate::chrome::snapshot::ClientShellSnapshot,
) -> bool {
    old.focused_tab_id != new.focused_tab_id
        || !old
            .tabs
            .iter()
            .map(|tab| tab.tab_id.as_str())
            .eq(new.tabs.iter().map(|tab| tab.tab_id.as_str()))
}

impl App {
    /// Recompute the projection for the current focus — finding 12:
    /// `focused_*` snapshot ids must track the focus without waiting
    /// for the next watcher publish. A publish whose projection inputs
    /// are unchanged skips the rebuild but still runs the cheap
    /// post-steps (title, copy/float focus).
    pub(super) fn refresh_projection(&mut self) {
        if self.focused != self.last_focus {
            self.previous_pane = self.last_focus.clone();
            self.last_focus = self.focused.clone();
            // Every focus path — keys, clicks, the shared layout document
            // or another client — lands here, so the agents list follows
            // the focused pane from this one comparison.
            self.shell_state.reveal_focused_agent = true;
        }
        if !self
            .project_cache
            .is_unchanged(&self.view, self.focused.as_deref())
        {
            let new_projection = self
                .project_cache
                .project(&self.view, self.focused.as_deref());
            let old_projection = std::mem::replace(&mut self.projection, new_projection);
            if tab_focus_or_order_changed(&old_projection.snapshot, &self.projection.snapshot) {
                self.shell_state.reveal_focused_tab = true;
            }
            self.presentation
                .project(&self.view, &mut self.projection, &self.waiting);
            self.probe
                .projections
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        self.update_tab_status(self.now());
        self.update_window_title();
        // The reference's `apply_active_snapshot` copy arms: pane loss
        // unarms the mode, focus re-arms/disarms it.
        self.sync_copy_focus();
        // Focus settled: park the old owner's float, restore the new
        // one's — before `reconcile_popups` sees the publish.
        self.sync_float_focus();
        // U1: the studio-mode latch follows the last good document and
        // the focus; a flip re-derives the sidebar width.
        self.sync_studio();
    }

    /// A watcher publish: refresh the projection + pane target policy.
    /// `watcher.view()` shares the publish's `Arc`s — the clone is a
    /// handful of refcount bumps, not a tree copy.
    pub(super) fn on_view(&mut self) {
        self.view = self.watcher.view();
        self.seen = self.view.version;
        self.retain_confirmed_layout();
        self.reconcile_published_view();
    }

    /// Everything `on_view` does after the view lands — split out so
    /// tests can publish a synthetic `self.view` through the same path.
    pub(super) fn reconcile_published_view(&mut self) {
        // U7: the entry decision reads the publish — settle it first so
        // the card and the layer are judged against this view.
        self.settle_opening(self.now());
        self.fence_chrome_core();
        self.follow_shared_focus();
        // The dead pane's context survives the reprojection for the
        // fallback below — a transient inventory view can precede the
        // layout write that names the real successor.
        let previous_tab = self.projection.snapshot.focused_tab_id.clone();
        let previous_workspace = self.projection.snapshot.focused_workspace_id.clone();
        // If the focused terminal fell out of the projected panes,
        // prefer a sibling in the same tab, then the same workspace —
        // never a global first pane that could ack another space's work
        // and never the client's own host, whose attach the core
        // refuses.
        // Resolve it before the single finalizing refresh below; the
        // provisional `project` runs through the incremental cache so an
        // unchanged endpoint never re-reads its structure.
        if self.mode == Mode::Normal && self.focus_missing() {
            let provisional = (!self
                .project_cache
                .is_unchanged(&self.view, self.focused.as_deref()))
            .then(|| {
                self.project_cache
                    .preview(&self.view, self.focused.as_deref())
            });
            let panes = match &provisional {
                Some(p) => &p.snapshot.panes,
                None => &self.projection.snapshot.panes,
            };
            // S1: the fallback never lands on the stage log — its keys
            // are dropped, so it cannot be where input belongs.
            let fallback = panes
                .iter()
                .find(|p| {
                    previous_tab.as_ref() == Some(&p.tab_id)
                        && !self.is_own_pane(&p.pane_id)
                        && !self.is_stage_terminal(&p.pane_id)
                })
                .or_else(|| {
                    panes.iter().find(|p| {
                        previous_workspace.as_ref() == Some(&p.workspace_id)
                            && !self.is_own_pane(&p.pane_id)
                            && !self.is_stage_terminal(&p.pane_id)
                    })
                })
                .or_else(|| {
                    panes.iter().find(|p| {
                        !self.is_own_pane(&p.pane_id) && !self.is_stage_terminal(&p.pane_id)
                    })
                })
                .map(|p| p.pane_id.clone());
            if fallback != self.focused {
                self.focused = fallback;
            }
        }
        // Probe bookkeeping precedes the reprojection so a fingerprint
        // the publish just invalidated cannot paint a stale Waiting.
        self.tick_waiting(self.now());
        self.refresh_projection();
        // U6c: the publish decides which machines want a version
        // probe — doc answers, reconnects and the 6h throttle all
        // live in `machine_versions`.
        self.sync_machine_versions();
        // W1: a stage down (or a missing document) asks the machine
        // for its ledger's studios — cached, refreshed at most every
        // 30 s and on every stage change.
        self.sync_studio_index();
        // S1: a stage start's focus is owed — keep the remembered
        // focus or move off the opening page, never the stage log.
        self.land_after_stage_start();
        // Focusing marks the active tab's panes seen — an idle agent
        // reports `Done` only while its workspace stayed background.
        self.mark_active_seen();
        self.sync_target();
        // A `studio new` just published its studio — focus its
        // producer. The publish also refreshed the projection, so the
        // new terminal's pane exists and `focus_pane` sticks.
        self.ns_focus_pending();
        self.pane.on_view(&self.view);
        if let Some(popup) = self.popup.as_mut() {
            popup.pane.on_view(&self.view);
        }
        if let Some(peek) = self.peek.as_mut() {
            peek.pane.on_view(&self.view);
        }
        self.sync_observers();
        self.reconcile_popups();
        // The `^E` cast editor's popup just closed (or never got to
        // close) — reload and validate the file it left. The Revise
        // dialog's `^E` requirement editor rides the same check.
        self.ns_editor_check();
        self.ns_build_editor_check();
        self.studio_editor_check();
        self.synthesize_alerts(self.now());
        // W2: the same publish seam feeds the studio events — a role's
        // question, a user prompt, a reply, a card, a ready studio or a
        // stopped stage announces once, keyed by its ledger seq.
        self.synthesize_studio_events(self.now());
        self.share_seen();
        self.reap_exited();
    }

    /// `self.focused` fell out of the publish's projected panes — gone
    /// from the inventory, closed by the layout document, or a
    /// popup-owned terminal the projection never lists.
    fn focus_missing(&mut self) -> bool {
        let Some(focused) = self.focused.clone() else {
            return true;
        };
        match self.view.inventory.get(&focused) {
            None => true,
            Some(term) => {
                if term.labels.contains_key(crate::app::popup::POPUP_LABEL) {
                    return true;
                }
                let own = self.project_cache.own_document(&self.view);
                if own.closed.get(&focused) == Some(&term.created_ms) {
                    return true;
                }
                // An exited pane stays "finished" in the inventory until it
                // is reaped, but the app layout has already dropped it.
                term.state == "finished"
                    && !self
                        .project_cache
                        .preview(&self.view, Some(&focused))
                        .snapshot
                        .panes
                        .iter()
                        .any(|pane| pane.pane_id == focused)
            }
        }
    }

    /// Request a surface draw; Source acknowledges only displayed output.
    pub(super) fn mark_active_seen(&mut self) {
        // A focus request alone cannot acknowledge unseen output. Source only
        // acknowledges a matching surface after it has actually been drawn.
        self.sched.mark_dirty(self.now());
    }
}

#[cfg(test)]
#[path = "view_sync_tests.rs"]
mod tests;

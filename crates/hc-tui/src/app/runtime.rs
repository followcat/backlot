// Derived from herdr src/client/shell/input.rs and composition.rs (Apache-2.0).
use super::wake::{self, WakeKind};
use super::*;

impl App {
    /// Draw once the scheduler permits. Backend errors propagate so the
    /// caller's RAII restores the terminal instead of silently
    /// continuing a broken frame loop.
    pub(super) fn maybe_draw<B: Backend + crate::ansi::FrameCursor>(
        &mut self,
        term: &mut Terminal<B>,
        now: Instant,
    ) -> Result<(), B::Error> {
        if !self.sched.due_now(now) {
            return Ok(());
        }
        // One shared geometry: the attach allocation IS the paintable
        // inner rect — no clipped last column, row or cursor.
        let size = term.size()?;
        self.area = (size.width, size.height);
        let layout = self.shell.layout_config().layout(
            size.width,
            size.height,
            self.shell_state.sidebar_collapsed,
            pane_geometry::focused_tab_count(self),
            self.sidebar_width,
        );
        let infos = pane_geometry::pane_infos(self, layout.pane_surface);
        // Zoom covers the rest of the focused tab: the reference resizes
        // only the zoomed pane, so each covered pane's lease stays alive
        // at its last allocation instead of dropping to headless size.
        let covered = self
            .projection
            .snapshot
            .focused_tab_id
            .as_ref()
            .and_then(|id| self.projection.layouts.get(id))
            .filter(|tab| tab.zoomed.is_some())
            .and_then(|tab| tab.root.as_ref())
            .map(|root| {
                root.terminals()
                    .into_iter()
                    .filter(|id| !infos.iter().any(|info| &info.id == id))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        // A lease outlives its tab for as long as the pane stays in the observer
        // LRU: the program keeps its size while hidden, so coming back resizes
        // nothing. The LRU, not the observer map: on a switch the pane retargets
        // and this frame draws before `sync_observers` opens the observer for
        // the tab just left (the focused terminal drops its own once painted).
        self.size_leases.retain(|id, _| {
            !id.is_empty()
                && self.view.inventory.get(id).is_some()
                && (infos.iter().any(|info| &info.id == id)
                    || covered.contains(id)
                    || self.observers.contains_key(id)
                    || self.observer_mru.contains(id)
                    || self.pane.target() == Some(id.as_str())
                    || self.floats.values().any(|entry| &entry.terminal == id))
        });
        // While a layout write is in flight, inventory can publish a new
        // member before the CAS lands — a transient un-zoomed split that
        // must not resize panes the confirmed layout will cover. Geometry
        // still draws; size claims wait for the write to settle.
        let settling = self.pending_actions > 0;
        for info in infos {
            let inner = self
                .displayed_pane(&info.id)
                .map(|pane| pane_geometry::inner_for(pane, &info, self.shell.pane_scrollbars).0)
                .unwrap_or_else(|| crate::chrome::panes::pane_inner_rect(info.rect, info.borders));
            if info.is_focused && !settling {
                self.pane.allocated(inner.width.max(1), inner.height.max(1));
            }
            if !settling && let Some(pane) = self.observers.get_mut(&info.id) {
                pane.allocated(inner.width.max(1), inner.height.max(1));
            }
            if info.id.is_empty() || self.view.inventory.get(&info.id).is_none() {
                continue;
            }
            // A new lease takes its size from this layout, which isn't final
            // while a write is in flight. Opened now, it would attach at the
            // default size and, as the newest typing viewer, resize the program.
            if settling && !self.size_leases.contains_key(&info.id) {
                continue;
            }
            let lease = self.size_leases.entry(info.id.clone()).or_insert_with(|| {
                let mut lease = pane::Pane::new(self.pane_tx.clone(), self.open_attach.clone());
                lease.set_host_facts(self.host_facts.clone());
                lease.allocated(inner.width.max(1), inner.height.max(1));
                lease.want(Some(Spec {
                    terminal: info.id,
                    role: Role::SizeLease,
                }));
                lease
            });
            if !settling {
                lease.allocated(inner.width.max(1), inner.height.max(1));
            }
        }
        for lease in self.size_leases.values_mut() {
            lease.on_view(&self.view);
        }
        // The same lifecycle ratatui's `try_draw` runs, except the
        // resolved host cursor is handed to the backend between compose
        // and apply, so `flush` can emit it inside the synchronized
        // block — `term.draw` would expose no hook between them.
        term.autoresize()?;
        {
            let mut frame = term.get_frame();
            draw::draw(self, &mut frame, now);
        }
        term.backend_mut()
            .set_frame_cursor(self.resolve_host_cursor());
        term.apply_buffer_with_cursor(None)?;
        if term.backend().emitted() {
            self.sched.drew(now);
            self.probe
                .draws
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        } else {
            self.sched.unarm();
            self.probe
                .elided_draws
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(())
    }

    /// Resolves the frame's cursor wish into the host-cursor state the
    /// backend emits inside the synchronized block — the reference's
    /// `resolve_host_cursor_state` (protocol/render_ansi.rs): a visible
    /// position updates the hidden-cursor memory, an explicit anchor
    /// keeps it, and with neither the memory (or bottom-right) applies
    /// with the terminal's own shape.
    pub(super) fn resolve_host_cursor(&mut self) -> crate::ansi::HostCursor {
        let wish = self.want_cursor;
        let (width, height) = self.area;
        let clamp = |(x, y): (u16, u16)| {
            (
                x.min(width.saturating_sub(1)),
                y.min(height.saturating_sub(1)),
            )
        };
        let shape = || crate::ansi::cursor_shape_param(wish.shape, wish.blinking);
        if let Some(position) = wish.pos.map(clamp) {
            self.last_visible_cursor = Some(position);
            return crate::ansi::HostCursor {
                x: position.0,
                y: position.1,
                visible: true,
                shape: shape(),
            };
        }
        if let Some(position) = wish.anchor.map(clamp) {
            return crate::ansi::HostCursor {
                x: position.0,
                y: position.1,
                visible: false,
                shape: shape(),
            };
        }
        let (x, y) = self
            .last_visible_cursor
            .map(clamp)
            .unwrap_or((width.saturating_sub(1), height.saturating_sub(1)));
        crate::ansi::HostCursor {
            x,
            y,
            visible: false,
            shape: 0,
        }
    }

    /// The loop: await input, watcher publishes, pane events and the
    /// one-shot deadline — nothing else wakes it. A draw backend error
    /// ends the run so the caller's RAII restores the terminal.
    pub async fn run<B: Backend + crate::ansi::FrameCursor>(
        &mut self,
        term: &mut Terminal<B>,
        events: &mut mpsc::Receiver<UiEvent>,
        replies: &mut mpsc::Receiver<crate::host::HostReply>,
    ) -> Result<(), B::Error> {
        self.seen = self.watcher.version();
        // The real terminal size seeds `area` — a `^B b` pressed before
        // the first Resize event must still see the true width.
        self.area = term
            .size()
            .map(|s| (s.width, s.height))
            .unwrap_or(self.area);
        self.on_view();
        self.sched.mark_dirty(Instant::now());
        let mut replies_open = true;
        let mut seen_open = true;
        let mut waiting_open = true;
        let mut pane_open = true;
        let mut image_read_done_open = true;
        let mut update_open = true;
        let mut need_pane_sync = true;
        while !self.quitting {
            let wake = self.wake_deadline();
            self.probe
                .wakeups
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if wake.is_some() {
                self.probe
                    .timers
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.sched.armed_timer();
            }
            let marks = self.sched.marks();
            // A wakeup alone is never a draw reason — each arm reports
            // whether it changed state the frame can show. `on_view`,
            // `on_operation` and `show_notice` mark the scheduler
            // themselves (seen-ack, published projection, notices).
            let mut dirty = false;
            let mut kind = WakeKind::None;
            tokio::select! {
                biased;
                ev = events.recv() => match ev {
                    Some(ev) => {
                        self.on_input(ev, self.area.0);
                        dirty = true;
                        kind = WakeKind::Input;
                    }
                    None => break, // input source ended: clean exit
                },
                reply = replies.recv(), if replies_open => match reply {
                    Some(first) => {
                        let batch = wake::drain_replies(replies, first, &mut replies_open);
                        dirty |= self.on_host_replies(batch);
                        kind = WakeKind::Replies;
                    }
                    None => replies_open = false,
                },
                _ = self.watcher.changed_since(self.seen) => {
                    self.on_view();
                    kind = WakeKind::View;
                },
                result = Self::operation_result(&mut self.operations) => {
                    self.on_operation(result);
                    kind = WakeKind::View;
                },
                report = Self::core_start_report(&mut self.core_start) => {
                    self.on_core_start(report);
                    kind = WakeKind::View;
                },
                report = Self::update_check_report(&mut self.update_check) => {
                    self.on_update_check(report);
                    kind = WakeKind::View;
                },
                ev = self.update_rx.recv(), if update_open => match ev {
                    Some(first) => {
                        self.on_update_event(first);
                        while let Ok(ev) = self.update_rx.try_recv() {
                            self.on_update_event(ev);
                        }
                        dirty = true;
                        kind = WakeKind::View;
                    }
                    None => update_open = false,
                },
                seen = self.seen_rx.recv(), if seen_open => match seen {
                    Some(first) => {
                        self.on_seen_result(first);
                        while let Ok(seen) = self.seen_rx.try_recv() {
                            self.on_seen_result(seen);
                        }
                        kind = WakeKind::Seen;
                    }
                    None => seen_open = false,
                },
                outcome = self.waiting_rx.recv(), if waiting_open => match outcome {
                    Some(first) => {
                        self.on_waiting_result(first);
                        while let Ok(outcome) = self.waiting_rx.try_recv() {
                            self.on_waiting_result(outcome);
                        }
                        kind = WakeKind::Waiting;
                    }
                    None => waiting_open = false,
                },
                ev = self.pane_rx.recv(), if pane_open => match ev {
                    Some(first) => {
                        dirty |= self.drain_pane_events(first, wake, events, &mut pane_open);
                        kind = WakeKind::Pane;
                    }
                    None => pane_open = false,
                },
                done = self.image_read_done_rx.recv(), if image_read_done_open => match done {
                    Some(first) => {
                        self.on_image_read_done(first);
                        while let Ok(done) = self.image_read_done_rx.try_recv() {
                            self.on_image_read_done(done);
                        }
                        dirty = true;
                        kind = WakeKind::Input;
                    }
                    None => image_read_done_open = false,
                },
                _ = async {
                    match wake {
                        Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
                        None => std::future::pending::<()>().await,
                    }
                }, if wake.is_some() => {
                    let now = Instant::now();
                    dirty |= self.pane_retry(now);
                    dirty |= self.expire_image_read(now);
                    dirty |= self.install_tick(now);
                    kind = WakeKind::Deadline;
                }
            }
            self.probe.wake_kinds[kind as usize].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // A liveness-only wake maintains nothing; a deadline that
            // already passed is taken by the next select's arm.
            if matches!(kind, WakeKind::None) {
                continue;
            }
            self.housekeeping(kind, wake, dirty);
            self.maybe_draw(term, Instant::now())?;
            self.post_draw_sync(kind, &mut need_pane_sync);
            self.probe.dirty_marks[kind as usize].fetch_add(
                self.sched.marks() - marks,
                std::sync::atomic::Ordering::Relaxed,
            );
            if self.probe.publish_panes {
                self.publish_panes();
            }
            if self.pane.attaches > 0 {
                self.probe
                    .attaches
                    .store(self.pane.attaches, std::sync::atomic::Ordering::Relaxed);
            }
        }
        // The input source can end while a coalesced draw still waits
        // on the frame cap: paint it now so `run` returns only once
        // the last processed state is the drawn one. The host terminal may
        // already be gone (the window closed), so a failed paint must not
        // skip the releases and shutdown below or fail a clean exit.
        let now = Instant::now();
        self.sched.force_due(now);
        let _ = self.maybe_draw(term, now);
        // Detach/quitting while keys are held: the pane's program saw
        // presses and must see their releases before the attach dies.
        // The source server drains delivered presses on detach, independently
        // of client leases subsequently consumed by UI bindings.
        self.release_pane_mouse();
        self.pane_rx.close();
        self.shutdown_panes().await;
        Ok(())
    }
}

impl App {
    /// An input drain failure is reported after terminal restoration on exit.
    pub fn failure(&self) -> Option<&str> {
        self.pane
            .close_failure()
            .or(self.background_failure.as_deref())
    }
}

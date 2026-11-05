// Derived from herdr src/client/shell/input.rs and composition.rs (Apache-2.0).
use super::*;

impl App {
    /// Draw once the scheduler permits. Backend errors propagate so the
    /// caller's RAII restores the terminal instead of silently
    /// continuing a broken frame loop.
    pub(super) fn maybe_draw<B: Backend>(
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
        let inner = draw::pane_inner(self, size.width, size.height);
        self.pane.allocated(inner.width.max(1), inner.height.max(1));
        term.draw(|frame| draw::draw(self, frame, now))?;
        self.apply_cursor();
        self.sched.drew(now);
        self.probe
            .draws
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    /// Applies the frame's cursor wish to the outer terminal: position
    /// was already set during draw; the style (shape + blink) is a
    /// backend cursor style, emitted only on change. `TermGuard`
    /// restores the terminal's own cursor on exit.
    pub(super) fn apply_cursor(&mut self) {
        let wish = self.want_cursor;
        // Source HostCursorState: explicit hidden anchor, else last visible
        // position, else bottom-right. Clamp memory after a resize.
        if let Some(position) = wish.pos {
            self.last_visible_cursor = Some(position);
        }
        let (width, height) = self.area;
        let (x, y) = wish
            .anchor
            .or(wish.pos)
            .or(self.last_visible_cursor)
            .unwrap_or((width.saturating_sub(1), height.saturating_sub(1)));
        let position = format!(
            "\x1b[{};{}H",
            y.min(height.saturating_sub(1)) + 1,
            x.min(width.saturating_sub(1)) + 1
        );
        let _ = (self.host_control)(position.as_bytes());
        let want = (
            wish.shape,
            wish.shape == hc_vt::CursorShape::Default || wish.blinking,
        );
        if want != self.applied_cursor {
            (self.cursor_sink)(wish.shape, wish.blinking);
            self.applied_cursor = want;
        }
        let _ = (self.host_control)(if wish.pos.is_some() {
            b"\x1b[?25h"
        } else {
            b"\x1b[?25l"
        });
    }

    /// The loop: await input, watcher publishes, pane events and the
    /// one-shot deadline — nothing else wakes it. A draw backend error
    /// ends the run so the caller's RAII restores the terminal.
    pub async fn run<B: Backend>(
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
        while !self.quitting {
            let wake = {
                let pane_wake = self.pane.next_wake();
                let draw_due = self.sched.deadline();
                let due = match (pane_wake, draw_due) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                due.into_iter()
                    .chain(self.notice.as_ref().map(|n| n.deadline))
                    .chain(self.next_tab_status)
                    .min()
            };
            self.probe
                .wakeups
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if let Some(deadline) = wake {
                self.probe
                    .timers
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.sched.armed_timer();
                tokio::select! {
                    ev = events.recv() => match ev {
                        Some(ev) => self.on_input(ev, self.area.0),
                        None => break, // input source ended: clean exit
                    },
                    reply = replies.recv(), if replies_open => {
                        if let Some(reply) = reply {
                            self.on_host_reply(reply);
                        } else {
                            replies_open = false;
                        }
                    },
                    _ = self.watcher.changed_since(self.seen) => self.on_view(),
                    ev = self.pane_rx.recv() => {
                        if let Some(ev) = ev {
                            self.pane.on_event(ev, Instant::now());
                        }
                    },
                    _ = tokio::time::sleep_until(deadline.into()) => {
                        self.pane.fire_retry(Instant::now());
                    }
                }
            } else {
                tokio::select! {
                    ev = events.recv() => match ev {
                        Some(ev) => self.on_input(ev, self.area.0),
                        None => break,
                    },
                    reply = replies.recv(), if replies_open => {
                        if let Some(reply) = reply {
                            self.on_host_reply(reply);
                        } else {
                            replies_open = false;
                        }
                    },
                    _ = self.watcher.changed_since(self.seen) => self.on_view(),
                    ev = self.pane_rx.recv() => {
                        if let Some(ev) = ev {
                            self.pane.on_event(ev, Instant::now());
                        }
                    },
                }
            }
            self.reconcile_key_routes();
            // Drained work is dirty; draw only if a frame slot is free.
            let now = Instant::now();
            if let Some(reason) = self.pane.take_input_rejection() {
                self.reject_input(&reason);
            }
            self.expire_notice(now);
            self.update_window_title();
            if self.next_tab_status.is_some_and(|due| now >= due) {
                self.update_tab_status(now);
            }
            self.sched.mark_dirty(now);
            self.maybe_draw(term, now)?;
            self.sync_target();
            self.sync_keyboard_report_all();
            self.sync_pixel_mouse();
            if self.pane.attaches > 0 {
                self.probe
                    .attaches
                    .store(self.pane.attaches, std::sync::atomic::Ordering::Relaxed);
            }
        }
        // Detach/quitting while keys are held: the pane's program saw
        // presses and must see their releases before the attach dies.
        // The source server drains delivered presses on detach, independently
        // of client leases subsequently consumed by UI bindings.
        self.release_pane_mouse();
        self.pane_rx.close();
        self.pane.shutdown().await;
        Ok(())
    }
}

//! Pane attach lifecycle: spawn/teardown, allocation and control
//! pushes, pending-input accounting. `impl Pane` continues here —
//! sibling files share the module's private state.
use super::*;
impl Pane {
    /// Opens a fresh attach for `spec` at the next generation.
    /// `resumed` marks wire recovery (no fresh ownership activity);
    /// `attempts` carries the backoff count a Down retry accumulated.
    /// A new incarnation drops `Encoded` leftovers — bytes bound to the
    /// dead wire are never replayed; `Key`/`Paste` items re-encode
    /// under the fresh post-paint VT.
    pub(super) fn spawn(&mut self, spec: Spec, resumed: bool, attempts: u32) {
        // All visible panes share an event channel. A wire identity is unique
        // across panes and reincarnations, including retired observer events.
        static NEXT_ROUTE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        self.generation = NEXT_ROUTE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.attaches += 1;
        // A disconnected batch must never lose a prefix but replay its tail.
        if self.pending.iter().any(|item| {
            matches!(
                item,
                Pending::Encoded(..) | Pending::Prepared(..) | Pending::Mouse(..)
            )
        }) {
            self.discard_pending_incarnation();
        }
        let (cols, rows) = self.allocated;
        let params = AttachParams {
            terminal: spec.terminal.clone(),
            observe: spec.role == Role::Observe,
            resize_notifications: true,
            quiet_human_release: spec.role == Role::Active,
            history: HISTORY,
            cols,
            rows,
            display: if spec.role != Role::Observe {
                self.host.display()
            } else {
                None
            },
            resumed,
            caller: None,
        };
        let generation = self.generation;
        let ev = self.ev.clone();
        let open = Arc::clone(&self.open);
        let (out, out_rx) = mpsc::channel(OUT_QUEUE);
        let task = tokio::spawn(attach_task(open, params, generation, ev));
        self.state = State::Opening {
            generation,
            spec,
            task,
            out,
            out_rx,
            attempts,
            resumed,
            requested_grid: (cols, rows),
        };
    }

    /// Drops the current attach attempt/stream. `graceful` spawns the
    /// CLOSE handshake when the Arc is already sole-owned; either way
    /// `Attach::drop` bounds the teardown — the program is never killed.
    pub(super) fn teardown(&mut self, graceful: bool) {
        let state = std::mem::replace(&mut self.state, State::Idle);
        match state {
            State::Opening { task, .. } => task.abort(),
            State::Live(live) => {
                leave_live(&live);
                if graceful && let Ok(attach) = Arc::try_unwrap(live.attach) {
                    tokio::spawn(async move {
                        let _ = attach.detach().await;
                    });
                }
            }
            State::Idle | State::Down { .. } | State::Gone { .. } => {}
        }
    }

    /// Parks the live attach for input-lease drain on a pane switch:
    /// it keeps its writer and the VT that encodes releases under this
    /// wire's live modes. `Some(route)` binds the attach's generation —
    /// a reincarnated terminal is a different route. `None` when
    /// nothing live could be parked.
    pub fn park_outgoing(&mut self) -> Option<u64> {
        self.sweep_parked();
        let State::Live(_) = &self.state else {
            return None;
        };
        let State::Live(mut live) = std::mem::replace(&mut self.state, State::Idle) else {
            unreachable!()
        };
        if live.await_paint {
            leave_live(&live);
            if !self.pending.is_empty() {
                self.input_rejection =
                    Some("Switched before queued input could reach a fresh terminal paint".into());
            }
            self.pending.clear();
            self.pending_bytes = 0;
            return None;
        }
        if let Some(vt) = self.vt.take() {
            let mut pending_input = VecDeque::new();
            for item in std::mem::take(&mut self.pending) {
                if let Some(id) = dispatch_id(&item) {
                    self.record_dispatch(id);
                }
                if let Pending::Control(out) = item {
                    pending_input.push_back(out);
                    continue;
                }
                held::Held::forget_parked(&mut self.parked, &item, self.generation);
                live.held.track(&item);
                match input::encode_pending(&vt, &item) {
                    Ok(bytes) if !bytes.is_empty() => {
                        pending_input.push_back(Out::human(&item, bytes))
                    }
                    Ok(_) => {}
                    Err(error) => self.close_error = Some(error),
                }
            }
            self.pending_bytes = 0;
            if live.spec.role == Role::Active {
                pending_input.push_back(Out::Focus(false));
            }
            let route = self.generation;
            self.parked.push(Parked {
                held: live.held,
                route,
                terminal: live.spec.terminal.clone(),
                attach: live.attach,
                out: live.out,
                vt,
                pending_input,
                pending_reply: live.pending_reply,
                reader: live.reader,
                writer: live.writer,
            });
            events::flush_parked_out(self.parked.last_mut().expect("just parked"));
            Some(route)
        } else {
            leave_live(&live); // no VT to encode under: park is useless
            None
        }
    }

    /// A release bound for a parked wire — a press went out on that
    /// attach before the pane switched away. `false` when no parked
    /// route matches: the caller falls back to the live wire (a press
    /// that flushed after reincarnation) or drops the lease.
    /// A Full writer parks the encoded release for WriterProgress —
    /// bounded, ordered, never silently dropped.
    pub fn lease_input(&mut self, route: u64, key: &crate::app::input::TerminalKey) -> bool {
        self.sweep_parked();
        let Some(index) = self.parked.iter().position(|p| p.route == route) else {
            return false;
        };
        let parked = &self.parked[index];
        let legacy = parked.vt.keyboard_negotiated_nothing();
        let bytes = input::key_event_from_terminal_key(key, legacy)
            .map(|event| parked.vt.key_event(&event))
            .transpose();
        let bytes = match bytes {
            Ok(bytes) => bytes.unwrap_or_default(),
            Err(error) => {
                self.close_error = Some(error.to_string());
                return false;
            }
        };
        const PARKED_PENDING_MAX: usize = PENDING_MAX_ITEMS + 64;
        if parked.out.is_closed() {
            return false;
        }
        if !bytes.is_empty() && parked.pending_input.len() >= PARKED_PENDING_MAX {
            self.close_error = Some("Pending key releases exceeded the bounded input route".into());
            return false;
        }
        let item = Pending::Key(key.clone());
        self.forget_held_elsewhere(&item, route);
        let parked = &mut self.parked[index];
        parked.held.track(&item);
        if !bytes.is_empty() {
            parked.pending_input.push_back(Out::human(&item, bytes));
            events::flush_parked_out(parked);
        }
        true
    }

    /// Whether a parked drain attach still holds `route`.
    pub fn parked_has(&self, route: u64) -> bool {
        self.parked.iter().any(|p| p.route == route)
    }

    /// Closes a parked route once its last lease released — strictly
    /// ordered: accepted pending input feeds the writer's channel in
    /// order, `out` drops, the writer finishes its queued sends, the
    /// transport-write fence confirms every DATA frame is on the wire
    /// (CLOSE rides the control queue and would otherwise overtake it),
    /// then the attach detaches and `RouteClosed` retires the entry.
    /// The task stays owned — `writer_abort` cancels a blocked writer
    /// on teardown since aborting `done` alone would only detach it.
    pub fn unpark(&mut self, route: u64) {
        let Some(parked) = self.take_parked(route) else {
            return;
        };
        self.begin_close(parked);
    }

    /// Test-only: injects bytes behind a parked route's writer queue —
    /// the state `lease_input` produces when the writer's channel is
    /// full (unreachable deterministically without socket backpressure).
    #[cfg(test)]
    pub(crate) fn push_parked_pending(&mut self, route: u64, bytes: Vec<u8>) {
        if let Some(p) = self.parked.iter_mut().find(|p| p.route == route) {
            p.pending_input.push_back(Out::Input(bytes));
        }
    }

    /// Removes a parked route whose wire died or failed: releases can
    /// no longer be delivered — the caller's leases fail on the next
    /// `lease_input` instead of parking a dead wire forever.
    pub fn remove_parked(&mut self, route: u64) {
        if let Some(parked) = self.take_parked(route) {
            parked.reader.abort();
            parked.writer.abort();
        }
    }

    pub(super) fn take_parked(&mut self, route: u64) -> Option<Parked> {
        let i = self.parked.iter().position(|p| p.route == route)?;
        Some(self.parked.remove(i))
    }

    /// Drops parked attaches whose writer died (closed `out`).
    fn sweep_parked(&mut self) {
        let mut i = 0;
        while i < self.parked.len() {
            if self.parked[i].out.is_closed() {
                let parked = self.parked.remove(i);
                parked.reader.abort();
                parked.writer.abort();
            } else {
                i += 1;
            }
        }
    }

    /// Retires finished close tasks so `closing` stays small —
    /// `RouteClosed` wakes us for exactly this; entries whose `done`
    /// finished drop here.
    pub(super) fn sweep_closing(&mut self) {
        self.closing.retain(|c| !c.done.is_finished());
    }

    /// A press actually dispatched to a wire — the generation that took
    /// it. Pending input flushes under a reincarnation, so the lease's
    /// true route is this record, not the generation named at press
    /// time. `None` means the press never reached a wire.
    pub(crate) fn dispatched_route(&self, key: &crate::app::input::TerminalKey) -> Option<u64> {
        self.dispatched
            .iter()
            .rev()
            .find(|(identity, _)| key.dispatch_id != 0 && *identity == key.dispatch_id)
            .map(|(_, route)| *route)
    }

    fn record_dispatch(&mut self, id: u64) {
        record_dispatch(&mut self.dispatched, id, self.generation);
    }

    /// The pane's allocated inner area changed (window resize, sidebar
    /// toggle). Active input and silent size holders may ask the core to match; a
    /// dropped send stays pending — never marked, never applied locally.
    pub fn allocated(&mut self, cols: u16, rows: u16) {
        if self.allocated == (cols, rows) {
            return;
        }
        if !matches!(self.state, State::Live(_)) {
            self.allocation_changed_disconnected = true;
        }
        self.allocated = (cols, rows);
        let State::Live(live) = &mut self.state else {
            return; // the spec change lands in the next attach's params
        };
        if live.spec.role == Role::Observe {
            return;
        }
        live.desired_size = Some((cols, rows));
        self.push_controls();
    }

    /// Test-only: the last layout allocation reported to this pane.
    #[cfg(test)]
    pub(crate) fn allocated_size(&self) -> (u16, u16) {
        self.allocated
    }

    /// Sends what fits of the coalesced controls: latest desired size and
    /// focus. `sent_size` and the local VT move only on a real send —
    /// a Full queue leaves everything armed for the progress wake.
    /// Human input: encoded and queued. Buffered while attaching,
    /// refused (visibly) while observing or down — never silently.
    /// The semantic key travels whole so generated text/shifted
    /// alternates encode against the live post-paint modes.
    pub fn key(&mut self, key: &crate::app::input::TerminalKey) -> bool {
        self.reset_scroll();
        self.push_pending(Pending::Key(key.clone()))
    }

    /// Pasted text: encoded against the terminal's paste mode.
    pub fn paste(&mut self, text: &[u8]) {
        self.reset_scroll();
        self.push_pending(Pending::Paste(text.to_vec()));
    }

    /// Pre-encoded protocol input (SGR mouse reports): queued like a
    /// key, delivered verbatim.
    pub fn input_encoded(&mut self, bytes: Vec<u8>) {
        self.reset_scroll();
        self.mouse_input(bytes);
    }

    /// Mouse motion does not reset local history; other mouse events do so
    /// at the caller, exactly like Source server/pane_input.rs.
    pub(crate) fn mouse_input(&mut self, bytes: Vec<u8>) {
        self.push_pending(Pending::Encoded(bytes, None, false));
    }

    pub(crate) fn mouse(&mut self, input: hc_vt::MouseInput) -> bool {
        if !matches!(self.state, State::Opening { .. } | State::Live(_)) {
            return false;
        }
        self.push_pending(Pending::Mouse(self.generation, input))
    }

    pub(crate) fn reset_scroll(&mut self) {
        if self.spec().is_some_and(|spec| spec.role == Role::Active)
            && let Some(vt) = self.vt.as_mut()
            && let Some(scroll) = vt.scrollbar()
        {
            vt.scroll_to_row(scroll.total.saturating_sub(scroll.len) as usize);
        }
    }

    pub(super) fn push_pending(&mut self, item: Pending) -> bool {
        // Previously held controls are older than this new input event.
        self.push_controls();
        if matches!(self.state, State::Gone { .. })
            || self.spec().is_none_or(|spec| spec.role != Role::Active)
        {
            self.dropped_input += 1;
            self.input_rejection = Some("Terminal input is unavailable in this view".into());
            return false;
        }
        let bytes = charge(&item);
        if self.pending.len() >= PENDING_MAX_ITEMS || self.pending_bytes + bytes > PENDING_MAX_BYTES
        {
            self.dropped_input += 1;
            self.input_rejection = Some("Pending terminal input exceeded the bounded queue".into());
            return false;
        }
        self.pending_bytes += bytes;
        self.pending.push_back(item);
        self.flush_pending();
        true
    }

    /// Outer-terminal focus changed — a real event, never synthetic.
    /// Only the focused (active) attach reports it; observers can't.
    pub fn focused(&mut self, focused: bool) {
        self.outer_focus = Some(focused);
        if !matches!(self.state, State::Live(_)) {
            self.focus_changed_disconnected = true;
        }
        if let State::Live(live) = &mut self.state
            && live.spec.role == Role::Active
        {
            live.desired_focus = Some(focused);
            self.push_controls();
        }
    }
}

pub(super) fn dispatch_id(item: &Pending) -> Option<u64> {
    match item {
        Pending::Key(key) if key.input.action == hc_vt::Action::Press && key.dispatch_id != 0 => {
            Some(key.dispatch_id)
        }
        Pending::Encoded(_, id, _) => *id,
        Pending::Prepared(_, key) => dispatch_id(&Pending::Key(key.clone())),
        _ => None,
    }
}
pub(super) fn record_dispatch(records: &mut VecDeque<(u64, u64)>, id: u64, generation: u64) {
    if records.len() >= PENDING_MAX_ITEMS {
        records.pop_front();
    }
    records.push_back((id, generation));
}

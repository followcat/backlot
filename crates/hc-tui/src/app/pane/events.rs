//! Pane event handling: attach-actor reports, stream output in wire
//! order, failure/retry parking, and the writer wake paths.
use super::*;
impl Pane {
    /// An attach actor or writer reported something; stale generations
    /// are dropped so a slow open can never paint the wrong terminal.
    /// Parked-lease-drain generations route to their own wire — first,
    /// so a parked attach whose generation still equals the current
    /// one (no respawn yet) is never mistaken for live output.
    pub fn on_event(&mut self, ev: PaneEv, now: Instant) {
        match ev {
            PaneEv::Out { generation, out } if self.parked_has(generation) => {
                self.on_parked_out(generation, out)
            }
            PaneEv::Ended { generation } if self.parked_has(generation) => {
                self.remove_parked(generation)
            }
            PaneEv::WriterProgress { generation } if self.parked_has(generation) => {
                // Queued parked releases retry in order.
                if let Some(parked) = self.parked.iter_mut().find(|p| p.route == generation) {
                    while let Some(bytes) = parked.pending_reply.pop_front() {
                        if let Err(error) = parked.out.try_send(Out::Reply(bytes)) {
                            if let Out::Reply(bytes) = error.into_inner() {
                                parked.pending_reply.push_front(bytes);
                            }
                            break;
                        }
                    }
                    flush_parked_out(parked);
                }
            }
            PaneEv::WriterFailed { generation, why } if self.parked_has(generation) => {
                // A dead wire's failure belongs to its terminal — never
                // to the pane's current target.
                let served = self
                    .parked
                    .iter()
                    .find(|p| p.route == generation)
                    .map(|p| p.terminal.as_str());
                if served == self.target() {
                    self.close_error = Some(format!(
                        "Input route failed: {}",
                        crate::sidebar::clean(&why)
                    ));
                }
                self.remove_parked(generation)
            }
            PaneEv::Opened { generation, attach } if generation == self.generation => {
                self.on_opened(attach)
            }
            PaneEv::Out { generation, out } if generation == self.generation => {
                self.on_output(out, now)
            }
            PaneEv::Failed {
                generation,
                why,
                unsupported,
            } if generation == self.generation => self.on_failed(why, unsupported, now),
            PaneEv::Ended { generation } if generation == self.generation => self.on_ended(now),
            PaneEv::WriterProgress { generation } if generation == self.generation => {
                // Parked replies retry first — automatic answers the
                // program waits on take precedence over human input.
                if let State::Live(live) = &mut self.state {
                    while let Some(reply) = live.pending_reply.pop_front() {
                        match live.out.try_send(Out::Reply(reply)) {
                            Ok(()) => {}
                            Err(mpsc::error::TrySendError::Full(out))
                            | Err(mpsc::error::TrySendError::Closed(out)) => {
                                if let Out::Reply(reply) = out {
                                    live.pending_reply.push_front(reply);
                                }
                                break;
                            }
                        }
                    }
                }
                self.flush_pending();
                self.push_controls();
            }
            PaneEv::RouteClosed { route, error } => {
                // The drain may finish after a retarget: its failure
                // only surfaces while the pane still serves the terminal
                // the drained wire belonged to.
                let served = self
                    .closing
                    .iter()
                    .find(|c| c.route == route)
                    .map(|c| c.terminal.as_str());
                if error.is_some() && served == self.target() {
                    self.close_error = error;
                }
                self.closing.retain(|c| c.route != route);
            }
            PaneEv::WriterFailed { generation, why } if generation == self.generation => {
                if let State::Live(live) = &self.state {
                    let spec = live.spec.clone();
                    leave_live(live);
                    self.arm_down(
                        spec,
                        format!("input: {}", crate::sidebar::clean(&why)),
                        0,
                        now,
                    );
                }
            }
            _ => {} // stale generation: a newer switch owns the pane
        }
    }

    /// One parked-lease-drain stream event: DATA feeds that wire's VT
    /// (a post-switch `CSI =0u` really does change what a later release
    /// encodes) and credit flows; automatic replies still answer — all
    /// isolated from the active paint, cursor, focus and geometry.
    /// A closed/failed wire ends the route.
    fn on_parked_out(&mut self, route: u64, out: Output) {
        match out {
            Output::Data(bytes) => {
                let Some(parked) = self.parked.iter_mut().find(|p| p.route == route) else {
                    return;
                };
                parked.vt.feed(&bytes);
                if parked.attach.consume(bytes.len()).is_err() {
                    self.remove_parked(route);
                    return;
                }
                match parked.vt.take_replies() {
                    Ok(replies) if !replies.is_empty() => {
                        if let Err(error) = parked.out.try_send(Out::Reply(replies)) {
                            let pending_bytes: usize =
                                parked.pending_reply.iter().map(Vec::len).sum();
                            if let Out::Reply(bytes) = error.into_inner() {
                                if parked.pending_reply.len() < 32
                                    && pending_bytes + bytes.len() <= PENDING_MAX_BYTES
                                {
                                    parked.pending_reply.push_back(bytes);
                                } else {
                                    self.close_error = Some(
                                        "Automatic replies exceeded a parked input route's bound"
                                            .into(),
                                    );
                                }
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        self.close_error = Some(format!("Parked terminal reply failed: {error}"))
                    }
                }
                self.pending_bells = self
                    .pending_bells
                    .saturating_add(parked.vt.take_bell_count());
            }
            Output::Closed(_) | Output::Failed(_) => self.remove_parked(route),
            Output::Resize { .. } | Output::Control { .. } => {}
        }
    }

    /// Decode in wire order; only the complete mode barrier releases queued input.
    pub(super) fn on_output(&mut self, out: Output, now: Instant) {
        let State::Live(live) = &mut self.state else {
            return;
        };
        match out {
            Output::Data(bytes) => {
                // A size holder needs neither an emulator nor query replies.
                // Credit still flows so it cannot block the program.
                if live.spec.role == Role::SizeLease {
                    if let Err(error) = live.attach.consume(bytes.len()) {
                        live.vt_broken = Some(format!("credit: {error}"));
                    }
                    return;
                }
                if !live.paint_started {
                    live.paint_started = true;
                    let mut vt = Vt::new(live.core_grid.0, live.core_grid.1, SCROLLBACK);
                    self.initial = seed_host(&mut vt, &self.host);
                    self.vt = Some(vt);
                }
                let Some(vt) = &mut self.vt else {
                    return;
                };
                if let Some(repaint) = &mut live.repaint {
                    match repaint.feed(vt, &bytes) {
                        Ok(true) => live.repaint = None,
                        Ok(false) => {}
                        Err(error) => {
                            live.repaint = None;
                            live.vt_broken = Some(format!("appearance restore: {error}"));
                        }
                    }
                } else {
                    vt.feed(&bytes);
                }
                if live.await_paint {
                    // Both supported core pins serialize this mode after all
                    // keyboard/paste state (viewers.rs::paint_state). DATA
                    // chunk boundaries are not initial-paint boundaries.
                    live.paint_tail.extend_from_slice(&bytes);
                    if live
                        .paint_tail
                        .windows(8)
                        .any(|part| part == b"\x1b[?9001h" || part == b"\x1b[?9001l")
                    {
                        live.await_paint = false;
                        live.paint_tail.clear();
                        if vt.size() != Ok(live.core_grid)
                            && let Err(error) = vt.resize(live.core_grid.0, live.core_grid.1)
                        {
                            live.vt_broken = Some(format!("resize: {error}"));
                        }
                    } else if live.paint_tail.len() > 7 {
                        let keep = live.paint_tail.len() - 7;
                        live.paint_tail.drain(..keep);
                    }
                }
                if let Err(e) = live.attach.consume(bytes.len()) {
                    live.vt_broken = Some(format!("credit: {e}"));
                }
                let bells = vt.take_bell_count();
                match vt.take_replies() {
                    // An observer never sends — but the reply buffer
                    // must still be drained or it overflows and marks
                    // the VT broken.
                    Ok(replies) if !replies.is_empty() => {
                        if live.spec.role == Role::Active {
                            self.push_reply(replies)
                        }
                    }
                    Ok(_) => {}
                    // Whole replies were dropped — the program's
                    // pending queries go unanswered; say so.
                    Err(e) => live.vt_broken = Some(format!("replies: {e}")),
                }
                self.pending_bells = self.pending_bells.saturating_add(bells);
                // Fresh VT built: input queued for this incarnation
                // encodes now — once, under the fresh modes.
                self.flush_pending();
            }
            Output::Resize { cols, rows } => {
                // Core grid news ahead of replacement paint — apply it
                // before the bytes arrive.
                let grid = (cols.max(1), rows.max(1));
                if live.spec.role == Role::Observe
                    && !live.await_paint
                    && let Some(vt) = &mut self.vt
                {
                    if let Some(repaint) = &mut live.repaint {
                        repaint.flush_tail(vt);
                    } else {
                        match repaint::Repaint::new(vt) {
                            Ok(repaint) => live.repaint = Some(repaint),
                            Err(error) => {
                                live.vt_broken = Some(format!("appearance snapshot: {error}"))
                            }
                        }
                    }
                }
                live.core_grid = grid;
                self.vt_grid = grid;
                if let Some(vt) = &mut self.vt
                    && !live.await_paint
                    && let Err(e) = vt.resize(grid.0, grid.1)
                {
                    live.vt_broken = Some(format!("resize: {e}"));
                }
            }
            Output::Closed(reason) => {
                // The core ended this stream (detach, exit, holder
                // loss): park until the next attempt — the inventory
                // view reconciles lost/finished from its side.
                use hc_proto::client::CloseReason;
                let spec = live.spec.clone();
                let why = match reason {
                    CloseReason::Empty => "the core closed the attach".to_string(),
                    CloseReason::Json(v) => {
                        let reason = v.get("reason").and_then(|r| r.as_str()).unwrap_or("closed");
                        format!("closed: {}", crate::sidebar::clean(reason))
                    }
                    CloseReason::Raw(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        format!("closed: {}", crate::sidebar::clean(&text))
                    }
                };
                leave_live(live);
                self.arm_down(spec, why, 0, now);
            }
            Output::Failed(reason) => {
                let spec = live.spec.clone();
                let why = format!("failed: {}", crate::sidebar::clean(&reason));
                leave_live(live);
                self.arm_down(spec, why, 0, now);
            }
            Output::Control { .. } => {}
        }
    }

    /// Queues an automatic reply; a Full writer parks it (bounded) and
    /// WriterProgress retries — replies are never silently dropped.
    pub(super) fn push_reply(&mut self, bytes: Vec<u8>) {
        const REPLY_MAX: usize = 32;
        let State::Live(live) = &mut self.state else {
            return;
        };
        match live.out.try_send(Out::Reply(bytes)) {
            Ok(()) => {}
            Err(e) => {
                if live.pending_reply.len() >= REPLY_MAX {
                    live.vt_broken = Some("automatic replies overflowed".into());
                    return;
                }
                if let Out::Reply(bytes) = e.into_inner() {
                    live.pending_reply.push_back(bytes);
                }
            }
        }
    }

    /// The open failed: bounded backoff (carrying prior attempts) before
    /// the next try. An `unsupported` answer means the core cannot
    /// attach at all — the pane parks Gone with the reason visible
    /// instead of retrying forever; the inventory view reconciles it.
    pub(super) fn on_failed(&mut self, why: String, unsupported: bool, now: Instant) {
        let State::Opening { spec, attempts, .. } = &self.state else {
            return;
        };
        let (spec, attempts) = (spec.clone(), *attempts);
        self.state = State::Idle;
        if unsupported {
            self.state = State::Gone {
                spec,
                reason: why,
                permanent: true,
            };
        } else {
            self.arm_down(spec, why, attempts, now);
        }
    }

    /// The wire ended mid-stream: the next retry reattaches with
    /// `resumed` so recovery doesn't stamp fresh size-ownership
    /// activity. Owned tasks are aborted so nothing outlives the pane.
    pub(super) fn on_ended(&mut self, now: Instant) {
        let State::Live(live) = &self.state else {
            return; // already Down/Gone — keep the recorded reason
        };
        let spec = live.spec.clone();
        leave_live(live);
        self.arm_down(spec, "the attach stream ended".into(), 0, now);
    }

    /// Parks `spec` in `Down` with exponential backoff.
    pub(super) fn arm_down(&mut self, spec: Spec, reason: String, attempts: u32, now: Instant) {
        if matches!(self.state, State::Live(_)) {
            self.discard_pending_incarnation();
        }
        let delay = RETRY_MIN
            .saturating_mul(1u32 << attempts.min(10))
            .min(RETRY_MAX);
        self.state = State::Down {
            spec,
            reason,
            retry_at: now + delay,
            attempts: attempts + 1,
        };
    }
}

/// Seeds a fresh VT with the probed host facts — the cell pixel pitch
/// for XTWINOPS answers and the host's default colors so a program's
/// `CellColor::Default` resolves against the host palette. The host's
/// `OSC 4` answers overlay the VT's *built-in* palette (like the
/// reference's `apply_host_terminal_theme`), and the post-seed colors
/// are returned for the pane's `initial` baseline.
fn seed_host(vt: &mut Vt, host: &crate::host::HostFacts) -> Option<hc_vt::Colors> {
    // Source client/terminal_geometry.rs uses 8x16 only as a local fallback.
    // It is never promoted into a measured attach Display.cell fact.
    let (w, h) = host.cell_px.unwrap_or((8, 16));
    vt.set_cell_px(w, h);
    let palette = vt
        .frame(0)
        .ok()
        .filter(|_| host.theme.palette.iter().any(Option::is_some))
        .map(|frame| {
            let mut p = frame.colors.palette;
            for (index, color) in host.theme.palette.iter().enumerate() {
                if let Some(color) = color {
                    p[index] = *color;
                }
            }
            p
        });
    if vt
        .set_default_colors(
            host.theme.foreground,
            host.theme.background,
            palette.as_ref(),
        )
        .is_err()
    {
        return None;
    }
    vt.frame(0).ok().map(|frame| frame.colors)
}

/// Keep switch-time focus behind accepted input and before later releases.
pub(super) fn flush_parked_out(parked: &mut Parked) {
    while let Some(out) = parked.pending_input.pop_front() {
        if let Err(error) = parked.out.try_send(out) {
            parked.pending_input.push_front(error.into_inner());
            break;
        }
    }
}

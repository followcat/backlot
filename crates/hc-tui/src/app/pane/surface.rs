//! Pane host facts, inventory recovery and surface access.
use super::*;

impl Pane {
    /// The probed host facts seed fresh VTs. Late color replies also
    /// update the live VT and its host baseline; libghostty preserves
    /// the program's OSC overrides until the program resets them.
    pub fn set_host_facts(&mut self, host: HostFacts) {
        self.host = host;
        if let State::Live(live) = &mut self.state
            && live.spec.role == Role::Active
            && let Some(display) = self.host.display()
            && live.sent_facts.as_ref() != Some(&display)
        {
            live.desired_facts = Some(display);
        }
        self.push_controls();
        let (Some(vt), Some(initial)) = (&mut self.vt, &mut self.initial) else {
            return;
        };
        if let Some((width, height)) = self.host.cell_px {
            vt.set_cell_px(width, height);
        }
        // Start from the host baseline, never from the program's live
        // palette: promoting an OSC override would corrupt reset targets.
        let mut baseline = initial.clone();
        if let Some(fg) = self.host.theme.foreground {
            baseline.foreground = fg;
        }
        if let Some(bg) = self.host.theme.background {
            baseline.background = bg;
        }
        for (index, color) in self.host.theme.palette.iter().enumerate() {
            if let Some(color) = color {
                baseline.palette[index] = *color;
            }
        }
        if let Err(error) = vt.set_default_colors(
            self.host.theme.foreground,
            self.host.theme.background,
            Some(&baseline.palette),
        ) {
            if let State::Live(live) = &mut self.state {
                live.vt_broken = Some(format!("host colors: {error}"));
            }
            return;
        }
        *initial = baseline;
    }

    /// The inputs `cell_style` needs: the seeded initial defaults and
    /// the host theme. `initial` is the VT's post-seed colors captured
    /// at creation; `None` when no VT exists yet or its first frame
    /// failed — matching the reference's `initial_default_*: Option`.
    pub fn paint(&self) -> Paint {
        Paint {
            initial_fg: self.initial.as_ref().map(|c| c.foreground),
            initial_bg: self.initial.as_ref().map(|c| c.background),
            initial_palette: self.initial.as_ref().map(|c| c.palette),
            host_fg: self.host.theme.foreground,
            host_bg: self.host.theme.background,
        }
    }

    /// The terminal+role currently wanted; `None` detaches. A spec
    /// change clears ALL pending input — bytes typed for terminal A
    /// never flush into terminal B.
    pub fn want(&mut self, spec: Option<Spec>) {
        let current = self.spec().cloned();
        if current == spec {
            return;
        }
        // A spec change retires the retained screen only when a *different*
        // terminal follows — re-attaching the same one keeps its last
        // paint until fresh bytes land.
        if spec.as_ref().map(|s| &s.terminal) != current.as_ref().map(|s| &s.terminal) {
            self.vt = None;
            self.incarnation = None;
            self.inventory_incarnation = None;
            self.pending.clear();
            self.pending_bytes = 0;
        }
        self.teardown(false);
        match spec {
            Some(spec) => self.spawn(spec, false, 0),
            None => self.state = State::Idle,
        }
    }

    /// The terminal the active spec addresses — the lease drain's
    /// "did the attach target change" check.
    pub(crate) fn target(&self) -> Option<&str> {
        self.spec().map(|s| s.terminal.as_str())
    }

    /// The current wire's generation — the lease-drain route id. Each
    /// spawn/reincarnation bumps it, so a lease bound to one attach can
    /// never release into a replacement wire for the same address.
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// The active spec, whatever state it's in.
    pub(super) fn spec(&self) -> Option<&Spec> {
        match &self.state {
            State::Opening { spec, .. } => Some(spec),
            State::Live(live) => Some(&live.spec),
            State::Down { spec, .. } | State::Gone { spec, .. } => Some(spec),
            State::Idle => None,
        }
    }

    /// The watcher published a view: reconcile the pane's target with
    /// the authoritative inventory. Lost/finished terminals are never
    /// (re)attached — the pane waits, marked, until the terminal is
    /// live again ("wait for restarted"). An offline inventory keeps
    /// the current state; its rows are stale, not proof of loss.
    pub fn on_view(&mut self, view: &View) {
        let Some(spec) = self.spec().cloned() else {
            return;
        };
        let term = view.inventory.get(&spec.terminal);
        let next = term.map(|term| (term.created_ms, term.pid));
        let changed = self.inventory_incarnation.is_some() && self.inventory_incarnation != next;
        self.inventory_incarnation = next;
        if !view.inventory.is_offline()
            && changed
            && next.is_some()
            && self.incarnation != next
            && matches!(self.state, State::Live(_) | State::Opening { .. })
        {
            self.teardown(false);
            self.vt = None;
            self.initial = None;
            self.discard_pending_incarnation();
            self.spawn(spec, true, 0);
            return;
        }
        // The inventory's grid is the authoritative size record when
        // the attach path negotiated no ordered RESIZE: active and
        // legacy-observer viewers track core size events here. A
        // negotiated path ignores it — stream ordering rules there.
        if let State::Live(live) = &mut self.state
            && !live.negotiated
            && let Some(term) = term
        {
            let grid = (term.cols.max(1), term.rows.max(1));
            if grid != live.core_grid {
                live.core_grid = grid;
                self.vt_grid = grid;
                if let Some(vt) = &mut self.vt
                    && !live.await_paint
                    && let Err(e) = vt.resize(grid.0, grid.1)
                {
                    live.vt_broken = Some(format!("resize: {e}"));
                }
                if live.spec.role == Role::Active {
                    live.mismatched_grid =
                        live.core_grid != live.sent_size.unwrap_or(self.allocated);
                }
            }
        }
        let gone_reason: Option<String> = match term {
            Some(term) => match classify(term) {
                // Live or starting: attachable, no reason to park.
                Life::Live | Life::Pending => None,
                Life::Finished => Some(match &term.reason {
                    Some(r) => format!("finished·{}", crate::sidebar::clean(r)),
                    None => "finished".into(),
                }),
                Life::Lost => Some(match &term.reason {
                    Some(r) => format!("lost·{}", crate::sidebar::clean(r)),
                    None => "lost — waiting for restart".into(),
                }),
                Life::Other(word) => Some(match &term.reason {
                    Some(r) => format!(
                        "{}·{}",
                        crate::sidebar::clean(&word),
                        crate::sidebar::clean(r)
                    ),
                    None => crate::sidebar::clean(&word),
                }),
            },
            None => Some("not in the inventory".into()),
        };
        match (&self.state, gone_reason) {
            // The terminal died out from under us: park it Gone —
            // including "not in the inventory" (a remote machine's
            // terminals vanish when its line drops). Input queued for
            // the DEAD program incarnation dies with it — a restarted
            // shell must never receive the old program's keystrokes.
            (State::Live(_) | State::Opening { .. }, Some(reason)) => {
                self.teardown(false);
                self.discard_pending_incarnation();
                self.state = State::Gone {
                    spec,
                    reason,
                    permanent: false,
                };
            }
            // Live or starting again after lost/removed: reattach —
            // `resumed`, since nothing about the user's ownership
            // changed; automatic recovery must not stamp fresh
            // size-ownership activity. A Down retry keeps its own
            // timer; its reattach is `resumed` too.
            (State::Gone { permanent, .. }, None) => {
                // Only a recoverable loss reattaches on inventory news —
                // an unsupported core stays parked no matter how often
                // the same-generation view republishes a live row.
                if !permanent {
                    self.spawn(spec, true, 0);
                }
            }
            // A Down retry pending becomes Gone so the banner reflects
            // the authoritative reason.
            (State::Down { .. }, Some(reason)) => {
                self.discard_pending_incarnation();
                self.state = State::Gone {
                    spec,
                    reason,
                    permanent: false,
                };
            }
            _ => {}
        }
    }

    /// The next instant the pane needs the loop awake (a Down retry).
    pub fn next_wake(&self) -> Option<Instant> {
        match &self.state {
            State::Down { retry_at, .. } => Some(*retry_at),
            _ => None,
        }
    }

    /// Fires a due retry: a fresh attach for the same spec — `resumed`,
    /// since nothing about the user's ownership changed.
    pub fn fire_retry(&mut self, now: Instant) {
        if let State::Down {
            spec,
            retry_at,
            attempts,
            ..
        } = &self.state
            && *retry_at <= now
        {
            let (spec, attempts) = (spec.clone(), *attempts);
            self.spawn(spec, true, attempts);
        }
    }

    /// The retained screen, if any — the draw layer clips/pads it.
    pub(crate) fn accepts_route(&self, route: u64) -> bool {
        route == self.generation && matches!(self.state, State::Live(_) | State::Opening { .. })
    }

    pub(crate) fn has_fresh_paint(&self) -> bool {
        matches!(&self.state, State::Live(live) if !live.await_paint && live.repaint.is_none())
            && self.vt.is_some()
    }

    pub fn vt(&mut self) -> Option<&mut Vt> {
        self.vt.as_mut()
    }

    pub(crate) fn vt_ref(&self) -> Option<&Vt> {
        self.vt.as_ref()
    }

    /// Pending human input items + dropped count, for the status line.
    pub fn input_backlog(&self) -> (usize, u64) {
        (
            self.pending
                .iter()
                .filter(|item| !matches!(item, Pending::Control(_)))
                .count(),
            self.dropped_input,
        )
    }

    /// One-line state for the status row.
    pub fn status_label(&self, now: Instant) -> String {
        match &self.state {
            State::Idle => "no terminal".into(),
            State::Opening { spec, .. } => format!("attaching {}…", spec.terminal),
            State::Live(live) => {
                let mut s = if live.spec.role == Role::Observe {
                    format!("observing {}", live.spec.terminal)
                } else {
                    live.spec.terminal.clone()
                };
                if live.mismatched_grid {
                    s.push_str(" · size may be owned by another viewer — click the size notice to take control");
                }
                if let Some(why) = &live.vt_broken {
                    s.push_str(&format!(" · {why}"));
                }
                s
            }
            State::Down {
                spec,
                reason,
                retry_at,
                ..
            } => {
                let secs = retry_at.saturating_duration_since(now).as_secs();
                format!("{} · {reason} — retry in {secs}s", spec.terminal)
            }
            State::Gone { spec, reason, .. } => format!("{} · {reason}", spec.terminal),
        }
    }
}

// Derived from herdr src/client/shell/input.rs and composition.rs (Apache-2.0).
use super::*;

impl App {
    /// Host-terminal facts: seeds every pane's VT (default colors,
    /// cell pitch) and feeds the attach `Display` fact.
    /// The spec the pane shows. Navigate is highlight-only per the
    /// reference `move_navigate_workspace`: moving the pending highlight
    /// changes the sidebar reveal, never the pane — the focused terminal
    /// keeps its active attach until Enter accepts a new workspace.
    pub(super) fn desired_spec(&self) -> Option<Spec> {
        self.focused.clone().map(|terminal| Spec {
            terminal,
            role: Role::Active,
        })
    }

    /// Keep the pane aimed at the desired spec and the sidebar's
    /// selection aligned in Normal mode.
    pub(super) fn sync_target(&mut self) {
        // Normal mode: the pane follows `focused`; if it vanished,
        // fall back to the projection's first pane (navigation order).
        if self.mode == Mode::Normal && self.focused.is_none() {
            self.focused = self
                .projection
                .snapshot
                .panes
                .first()
                .map(|p| p.pane_id.clone());
        }
        // Retargeting keeps held presses bound to their original wire:
        // the outgoing attach parks for lease drain instead of getting
        // invented releases or stranding the program's key state.
        let desired = self.desired_spec();
        if desired.as_ref().map(|s| s.terminal.as_str()) != self.pane.target() {
            self.release_pane_mouse();
            let outgoing = self.pane.generation();
            let parked = self.pane.park_outgoing();
            let outgoing_leased = self
                .pane_leases
                .forwarded()
                .any(|l| self.lease_route(l) == outgoing);
            if !outgoing_leased
                && let Some(route) = parked
                && !self.pane.parked_held(route)
            {
                // Even a mouse-only switch must write accepted original-target
                // input/releases before its close instead of aborting the writer.
                self.pane.unpark(route);
            }
            if outgoing_leased && parked.is_none() {
                // No live wire left to park (attach died/opening): presses
                // bound to it are undeliverable — drop those leases.
                let removed: Vec<_> = self
                    .pane_leases
                    .forwarded()
                    .filter(|lease| self.lease_route(lease) == outgoing)
                    .map(|lease| key_leases::InputLeaseKey::new(0, &lease.key))
                    .collect();
                for key in removed {
                    self.pane_leases.remove(&key);
                }
            }
            // Retire the old target before applying the new target's geometry:
            // opening at the previous pane's size can resize its program and
            // race a concurrent visible size holder before the first draw.
            self.pane.want(None);
            if desired.is_some() {
                let (cols, rows) = self.area;
                let inner = pane_geometry::pane_inner(self, cols, rows);
                self.pane.allocated(inner.width.max(1), inner.height.max(1));
            }
        }
        self.pane.want(desired);
        // Focus restores (navigate cancel, sidebar clicks) re-arm Copy
        // when the copy pane is focused again — the reference's
        // `apply_active_snapshot` arm.
        self.sync_copy_focus();
    }

    /// Synthesized releases for pane-bound presses on real detach —
    /// the reference's `release_input_leases`: a program that saw a
    /// press must see its release when the UI is gone for good.
    /// Plain-text presses (IME/text commits) never get invented
    /// releases; parked wires drain through their own routes.
    pub(super) fn release_pane_leases(&mut self) {
        self.release_pane_mouse();
        let current = self.pane.generation();
        let mut routes = std::collections::HashSet::new();
        for lease in self.pane_leases.remove_source(0) {
            routes.insert(self.lease_route(&lease));
            let plain_text_press = lease.key.text_commit.is_some()
                && lease.key.vt_bytes.first().is_some_and(|b| *b != 0x1b);
            if plain_text_press {
                continue;
            }
            let release = Self::lease_release(&lease.key, None);
            let route = self.lease_route(&lease);
            if self.pane.parked_has(route) {
                self.pane.lease_input(route, &release);
            } else if route == current && self.pane.accepts_route(route) {
                self.pane.key(&release);
            } else {
                self.pane.cancel_undispatched_key(&lease.key);
            }
        }
        for route in routes {
            self.unpark_drained_route(route);
        }
    }

    /// The wire a leased press actually took: the dispatch record
    /// wins (a press queued while attaching flushes onto the attach
    /// that finally lands), the press-time generation otherwise.
    pub(super) fn lease_route(&self, lease: &PaneLease) -> u64 {
        self.pane
            .dispatched_route(&lease.key)
            .unwrap_or(lease.target)
    }

    /// The release a leased press emits. A VT release report already
    /// names its key and shifted character exactly, EXCEPT around a
    /// non-Latin Ctrl chord: Ctrl alone resolves to the physical key,
    /// so a modifier let go before the key flips between the layout
    /// character and that key — it is released as the stored press's
    /// code with the incoming modifiers. Every other release forwards
    /// as the report arrived; quit-synthesis flips the stored press.
    pub(crate) fn lease_release(
        stored: &input::TerminalKey,
        incoming: Option<&input::TerminalKey>,
    ) -> input::TerminalKey {
        let Some(incoming) = incoming else {
            let mut release = stored.clone();
            release.input.action = hc_vt::Action::Release;
            release.text_commit = None;
            return release;
        };
        let layout_chord =
            incoming.input.key != stored.input.key && incoming.identity() == stored.identity();
        if layout_chord {
            let mut release = stored.clone();
            release.input.mods = incoming.input.mods;
            release.input.action = hc_vt::Action::Release;
            release.text_commit = None;
            release.vt_bytes = incoming.vt_bytes.clone();
            release
        } else {
            incoming.clone()
        }
    }

    /// Closes a leased press with a real release: same-wire releases
    /// go through the live pane; a parked route gets its release over
    /// the drain attach, which closes once its last lease drains. An
    /// unmatched release — or one whose wire is gone — is dropped: an
    /// orphan release is never replayed into any program.
    pub(super) fn release_pane_lease(&mut self, incoming: &input::TerminalKey) -> bool {
        let Some(lease) = self.take_pane_lease(incoming) else {
            return false;
        };
        let key_leases::InputLease::Forwarded(lease) = lease else {
            self.prefix.release_swallowed(incoming);
            return true;
        };
        let release = Self::lease_release(&lease.key, Some(incoming));
        let route = self.lease_route(&lease);
        if self.pane.parked_has(route) {
            self.pane.lease_input(route, &release);
        } else if self.pane.accepts_route(route) {
            self.pane.key(&release);
        } else {
            self.pane.cancel_undispatched_key(&lease.key);
        }
        self.unpark_drained_route(route);
        true
    }

    /// Closes a parked route once no lease still names it.
    pub(super) fn unpark_drained_route(&mut self, route: u64) {
        let routes: Vec<u64> = self
            .pane_leases
            .forwarded()
            .map(|l| self.lease_route(l))
            .collect();
        if self.pane.parked_has(route) && !routes.contains(&route) && !self.pane.parked_held(route)
        {
            self.pane.unpark(route);
        }
    }
}

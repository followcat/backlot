//! The opening page's App wiring (U2): when it shows, the keys it
//! owns and what they run. Everything drawn lives in
//! `crate::opening`; the page is an overlay on the pane area, like
//! the fullscreen panel — the terminals beneath stay attached.

use hc_vt::Key;

use super::App;
use super::input::TerminalKey;

impl App {
    /// The first `backlot` read has landed: a value, an unwritten
    /// document or an error — anything but a fetch still in flight.
    pub(crate) fn document_settled(&self) -> bool {
        self.view
            .documents
            .get(&crate::panel::document_key())
            .is_some_and(|doc| !doc.fetching)
    }

    /// The entry gate (U7): which screen to show is unknowable until
    /// the first `backlot` read lands, so the client decides once —
    /// and draws neither the card nor the layer before then, so a
    /// client with studios never flashes the welcome card. The wait
    /// is capped at [`crate::opening::SETTLE_CAP`] from the first
    /// decision pass (the first connect, in practice); after it the
    /// client judges with what it has. Clearing `cap` leaves no timer
    /// behind.
    pub(crate) fn settle_opening(&mut self, now: std::time::Instant) {
        if self.opening.settled {
            return;
        }
        let cap = *self
            .opening
            .cap
            .get_or_insert(now + crate::opening::SETTLE_CAP);
        if !self.document_settled() && now < cap {
            return;
        }
        self.opening.settled = true;
        self.opening.cap = None;
        // S0: the bar's face is the lot pill — the start policy, not
        // the settle, decides whether it opens expanded (`expanded`,
        // or `when_waiting` with U > 0, applied in `sync_lot` once
        // the first summary's counts land). The settle only arms it
        // and floors the mode at collapsed.
        if !self.plain {
            if self.hollywood.mode == crate::hollywood::LayerMode::Hidden {
                self.hollywood.mode = crate::hollywood::LayerMode::Collapsed;
            }
            if !self.studio.lot.start_applied {
                self.studio.lot.start_pending = true;
            }
        }
        self.sched.mark_dirty(now);
    }

    /// The page paints over the pane area while there is no studio —
    /// no `backlot` document yet, or a good one with zero studios.
    /// `--plain` never shows it; Esc dismisses for the session, and
    /// `new_workspace` brings it back. It never covers a studio: with
    /// one, `new_workspace` opens the new-studio wizard instead.
    pub(crate) fn opening_visible(&self) -> bool {
        !self.plain && self.opening.settled && !self.opening.dismissed && self.studioless()
    }

    /// No studio exists on any machine: this core's document is
    /// missing, unreadable or foreign — the start state — and no
    /// added machine's document lists one. A studio that arrives from
    /// another machine closes the card like a local one does.
    pub(crate) fn studioless(&self) -> bool {
        crate::panel::data::studio_count(&self.panel_summaries()) == 0
    }

    /// What the page draws: `[hollywood]` gates `h`, a live local
    /// `backlot-stage` terminal gates `s` — the panel's own reads.
    pub(crate) fn opening_opts(&self) -> crate::opening::OpeningOpts {
        crate::opening::OpeningOpts {
            hollywood: self.has_hollywood_cfg,
            stage_running: crate::panel::stage_running(&self.view, None),
        }
    }

    /// `new_workspace` outside `--plain`: the page comes back. With a
    /// studio the action never lands here — it opens the wizard.
    pub(crate) fn opening_show(&mut self) {
        self.opening.dismissed = false;
    }

    // ---------- the one-time animation ----------

    /// The frame to paint this draw. The run starts on the page's
    /// first draw and never restarts: after it played — or the page
    /// was left — this answers `None`, the static page.
    pub(crate) fn opening_frame(&mut self) -> Option<u8> {
        (!self.opening.anim.done).then_some(self.opening.anim.frame)
    }

    /// Arm the next frame while the run is live; an idle page arms
    /// nothing, so the client parks on real events.
    pub(crate) fn arm_opening_anim(&mut self, now: std::time::Instant) {
        if self.opening.anim.done {
            self.opening_anim_deadline = None;
        } else {
            self.opening_anim_deadline
                .get_or_insert(now + crate::opening::FRAME);
        }
    }

    /// The frame deadline fired: advance, or finish on the last frame.
    /// The draw after it re-arms only while frames remain.
    pub(crate) fn tick_opening(&mut self, now: std::time::Instant) {
        if !self.opening_anim_deadline.is_some_and(|due| due <= now) {
            return;
        }
        self.opening_anim_deadline = None;
        let anim = &mut self.opening.anim;
        if anim.frame + 1 >= crate::opening::OPENING_FRAMES - 1 {
            anim.frame = crate::opening::OPENING_FRAMES - 1;
            anim.done = true;
        } else {
            anim.frame += 1;
        }
        self.sched.mark_dirty(now);
    }

    /// Esc, and embedders that draw their own opening frame: hide the
    /// page for this client session and finish its one-time run.
    pub fn dismiss_opening(&mut self) {
        self.opening.dismissed = true;
        self.opening_finish_anim();
    }

    /// Leaving the page before the run ends finishes it: no resize,
    /// Esc-and-back or later redraw ever replays it.
    pub(crate) fn opening_finish_anim(&mut self) {
        self.opening.anim.done = true;
        self.opening_anim_deadline = None;
    }

    /// A key while the page is up. It owns every key like the focused
    /// panel does — the studio keys, the panel chord and Hollywood's
    /// toggle already took theirs in routing; only the page's own
    /// keys act here.
    pub(crate) fn opening_key(&mut self, key: &TerminalKey) {
        match key.input.key {
            Key::Escape => {
                // Leaving finishes the one-time run: returning to the
                // page later shows it static.
                self.dismiss_opening();
            }
            Key::Char('n') if key.input.mods == hc_vt::Mods::NONE => self.opening_new_studio(),
            Key::Char('h') if key.input.mods == hc_vt::Mods::NONE => {
                if self.has_hollywood_cfg {
                    // H4's input line, opened over the page: a line
                    // sent here asks Hollywood for a `studio-new`
                    // card; the card lands in the expanded layer.
                    self.hollywood.mode = crate::hollywood::LayerMode::Expanded;
                    self.hollywood.editing = true;
                    self.hollywood.selected = None;
                    self.hollywood.scroll = 0;
                }
            }
            Key::Char('s') if key.input.mods == hc_vt::Mods::NONE => self.opening_start_stage(),
            Key::Char('q') if key.input.mods == hc_vt::Mods::NONE => self.quitting = true,
            _ => {}
        }
    }

    /// `n`: the native New Studio dialog (U6b) — shared by the
    /// card's `n`, the Call Sheet's `n`/`+ new studio` and
    /// `new_workspace` over an existing studio. Over a studio its
    /// machine is the preselected target; the page goes away on its
    /// own when the create's publish lifts the mode latch.
    pub(super) fn opening_new_studio(&mut self) {
        let machine = self
            .studio
            .current
            .as_ref()
            .and_then(|key| key.machine.clone());
        self.new_studio_open(machine);
    }

    /// `s`: `backlot stage start` on this core — U6a starts stages
    /// only locally; a stage on a chosen machine is U6b's.
    fn opening_start_stage(&mut self) {
        if crate::panel::stage_running(&self.view, None) {
            return;
        }
        let mut argv = vec!["stage".to_string(), "start".to_string()];
        argv.extend(self.panel_endpoint_args.iter().cloned());
        self.run_panel_action(crate::panel::PanelAction {
            label: "stage start".into(),
            argv,
            machine: None,
        });
    }
}

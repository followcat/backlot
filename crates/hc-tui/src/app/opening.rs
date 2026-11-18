//! The opening page's App wiring (U2): when it shows, the keys it
//! owns and what they run. Everything drawn lives in
//! `crate::opening`; the page is an overlay on the pane area, like
//! the fullscreen panel — the terminals beneath stay attached.

use std::collections::BTreeMap;

use hc_vt::Key;

use super::input::TerminalKey;
use super::popup::{POPUP_LABEL, PopupPending};
use super::{App, creation::program_env};
use crate::operations::Action;

/// POSIX single-quote escaping for one shell word — a `'` becomes
/// `'\''`: close the quote, literal quote, reopen. Every word the
/// wizard line hands the shell passes through it.
fn sh_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// The wizard's command line inside the popup: `backlot studio new
/// --interactive` plus the endpoint flags — a pure function so tests
/// assert the argv without a `current_exe`.
fn wizard_line(exe: &str, endpoint_args: &[String]) -> String {
    let mut line = format!("{} studio new --interactive", sh_quote(exe));
    for arg in endpoint_args {
        line.push_str(&format!(" {}", sh_quote(arg)));
    }
    line
}

/// The wizard's draft is meant to be read: a `permission_profile`
/// line already wraps at the default half-width popup. Ask for the
/// large-popup size — still frame-clamped, so narrow terminals get
/// the same shape relative to their pane.
const WIZARD_POPUP: crate::popup_size::PopupSize = crate::popup_size::PopupSize::Percent(80);

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

    /// `n`: `backlot studio new --interactive` in a one-shot floating
    /// terminal — the `type = "popup"` keybind path, shared by the
    /// card's `n`, the layer's new-studio frame and `new_workspace`
    /// over an existing studio. When the wizard created a studio the
    /// publish lifts the mode latch and the page goes away on its own.
    pub(super) fn opening_new_studio(&mut self) {
        if self
            .popup_pending
            .as_ref()
            .is_some_and(|pending| pending.persistent)
        {
            return;
        }
        if self.popup.is_some() || self.popup_pending.is_some() {
            self.dismiss_popup();
        }
        let params = match self.wizard_params() {
            Ok(params) => params,
            Err(message) => {
                self.show_notice("New studio", &message);
                return;
            }
        };
        self.popup_pending = Some(PopupPending {
            width: Some(WIZARD_POPUP),
            height: Some(WIZARD_POPUP),
            persistent: false,
            owner: String::new(),
            command: String::new(),
        });
        self.submit_action(Action::RunPopup { params });
    }

    /// The wizard's `terminal.run` parameters — split out so tests
    /// assert them without a wire. `on` is never set: the wizard must
    /// write where `backlot <endpoint args> studio new` writes, and
    /// that is the host the popup itself lands on.
    ///
    /// Local endpoint: this exe, this cwd — the user's launch
    /// directory is the wizard's repository default — and the
    /// client's `BACKLOT_STATE_DIR` forwarded (the spawned terminal
    /// inherits the core's env, not ours).
    ///
    /// Remote endpoint: the popup already runs on the core's host —
    /// the SSH target — where this exe's path, this cwd and
    /// `--remote <target>` all mean nothing. There `backlot
    /// --session <s>` (from that host's PATH) reaches the same
    /// ledger: `link::state_dir` keys the base dir by the core id
    /// `hello` reports, and the same core answers `--session`. The
    /// line says so when `backlot` is missing rather than failing
    /// silently.
    pub(crate) fn wizard_params(&self) -> Result<hc_proto::terminal::RunParams, String> {
        let mut env = program_env();
        let (line, cwd) = if self.panel_endpoint_args.iter().any(|a| a == "--remote") {
            // Drop `--remote <target>` — on that host the pair is a
            // loop back out. Everything else (`--session`) survives.
            let mut args = Vec::with_capacity(self.panel_endpoint_args.len());
            let mut skip = false;
            for arg in &self.panel_endpoint_args {
                if skip {
                    skip = false;
                    continue;
                }
                if arg == "--remote" {
                    skip = true;
                    continue;
                }
                args.push(arg.clone());
            }
            (
                format!(
                    "if ! command -v backlot >/dev/null 2>&1; then \
                     echo 'studio new needs backlot on this host — \
                     install it or run it where it is'; \
                     exec ${{SHELL:-sh}}; fi; {}",
                    wizard_line("backlot", &args)
                ),
                None,
            )
        } else {
            if let Ok(dir) = std::env::var("BACKLOT_STATE_DIR") {
                env.insert("BACKLOT_STATE_DIR".into(), dir);
            }
            let exe =
                std::env::current_exe().map_err(|_| "cannot find this executable".to_string())?;
            (
                wizard_line(&exe.to_string_lossy(), &self.panel_endpoint_args),
                std::env::current_dir()
                    .ok()
                    .map(|dir| dir.to_string_lossy().into_owned()),
            )
        };
        let mut params = hc_proto::terminal::RunParams {
            line: Some(line),
            rm: true,
            keep: false,
            no_notify: true,
            no_parent: true,
            env,
            labels: BTreeMap::from([(POPUP_LABEL.into(), "1".into())]),
            display: self.host_facts.display(),
            cwd,
            ..Default::default()
        };
        // The program's pty must start at the popup's cells, like
        // `spawn_popup`: left unset it takes the client's display and
        // the wizard's output never scrolls into the visible rows. The
        // size matches the `PopupPending` request — the same geometry
        // `on_popup_spawned` resolves for the frame.
        if let Some(geometry) = crate::popup_size::resolve_popup_geometry(
            Some(WIZARD_POPUP),
            Some(WIZARD_POPUP),
            self.frame_area(),
        ) {
            params.size = Some(hc_proto::terminal::InitialSize {
                cols: geometry.terminal_cols,
                rows: geometry.terminal_rows,
            });
        }
        Ok(params)
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

#[cfg(test)]
#[path = "opening_tests.rs"]
mod tests;

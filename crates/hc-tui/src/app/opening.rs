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

/// The wizard's command line inside the popup: `backlot studio new
/// --interactive` plus the endpoint flags — a pure function so tests
/// assert the argv without a `current_exe`.
fn wizard_line(exe: &str, endpoint_args: &[String]) -> String {
    let mut line = format!("'{exe}' studio new --interactive");
    for arg in endpoint_args {
        line.push_str(&format!(" '{arg}'"));
    }
    line
}

impl App {
    /// The page paints over the pane area while there is no studio —
    /// no `backlot` document yet, or a good one with zero studios —
    /// and while `new_workspace` has forced it up over a live studio.
    /// `--plain` never shows it; Esc dismisses for the session.
    pub(crate) fn opening_visible(&self) -> bool {
        !self.plain && !self.opening.dismissed && (self.opening.forced || self.studioless())
    }

    /// No studio exists: the document is missing, unreadable or
    /// foreign — the start state — or a good one lists none.
    pub(crate) fn studioless(&self) -> bool {
        self.panel_summary().is_none_or(|s| s.studios.is_empty())
    }

    /// What the page draws: `[hollywood]` gates `h`, a live
    /// `backlot-stage` terminal gates `s` — the panel's own reads.
    pub(crate) fn opening_opts(&self) -> crate::opening::OpeningOpts {
        crate::opening::OpeningOpts {
            hollywood: self.has_hollywood_cfg,
            stage_running: crate::panel::stage_running(&self.view),
        }
    }

    /// `new_workspace` outside `--plain`: the page comes back — the
    /// new-studio flow over an existing studio too.
    pub(crate) fn opening_show(&mut self) {
        self.opening.dismissed = false;
        self.opening.forced = !self.studioless();
    }

    /// A key while the page is up. It owns every key like the focused
    /// panel does — the studio keys, the panel chord and Hollywood's
    /// toggle already took theirs in routing; only the page's own
    /// keys act here.
    pub(crate) fn opening_key(&mut self, key: &TerminalKey) {
        match key.input.key {
            Key::Escape => {
                self.opening.dismissed = true;
                self.opening.forced = false;
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
    /// terminal — the `type = "popup"` keybind path. When the wizard
    /// created a studio the publish lifts the mode latch and the page
    /// goes away on its own.
    fn opening_new_studio(&mut self) {
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
        let Ok(exe) = std::env::current_exe() else {
            self.show_notice("New studio", "cannot find this executable");
            return;
        };
        // The spawned terminal inherits the core's env, not ours —
        // hand it a state dir override when this client has one so
        // the wizard writes the same ledgers `backlot` reads.
        let mut env = program_env();
        if let Ok(dir) = std::env::var("BACKLOT_STATE_DIR") {
            env.insert("BACKLOT_STATE_DIR".into(), dir);
        }
        let mut params = hc_proto::terminal::RunParams {
            line: Some(wizard_line(
                &exe.to_string_lossy(),
                &self.panel_endpoint_args,
            )),
            rm: true,
            keep: false,
            no_notify: true,
            no_parent: true,
            env,
            labels: BTreeMap::from([(POPUP_LABEL.into(), "1".into())]),
            display: self.host_facts.display(),
            // The wizard's "current directory" default is the user's:
            // this client's launch cwd, not the core's.
            cwd: std::env::current_dir()
                .ok()
                .map(|dir| dir.to_string_lossy().into_owned()),
            ..Default::default()
        };
        if let Some(machine) = self
            .focused
            .as_deref()
            .and_then(|s| s.split_once('/').map(|(m, _)| m.to_owned()))
        {
            params.on = Some(machine);
        }
        // The program's pty must start at the popup's cells, like
        // `spawn_popup`: left unset it takes the client's display and
        // the wizard's output never scrolls into the visible rows.
        if let Some(geometry) =
            crate::popup_size::resolve_popup_geometry(None, None, self.frame_area())
        {
            params.size = Some(hc_proto::terminal::InitialSize {
                cols: geometry.terminal_cols,
                rows: geometry.terminal_rows,
            });
        }
        self.popup_pending = Some(PopupPending {
            width: None,
            height: None,
            persistent: false,
            owner: String::new(),
            command: String::new(),
        });
        self.submit_action(Action::RunPopup { params });
    }

    /// `s`: `backlot stage start` through the panel's injected
    /// runner — off the event loop like every panel action.
    fn opening_start_stage(&mut self) {
        if crate::panel::stage_running(&self.view) {
            return;
        }
        let mut argv = vec!["stage".to_string(), "start".to_string()];
        argv.extend(self.panel_endpoint_args.iter().cloned());
        self.run_panel_action(crate::panel::PanelAction {
            label: "stage start".into(),
            argv,
        });
    }
}

#[cfg(test)]
#[path = "opening_tests.rs"]
mod tests;

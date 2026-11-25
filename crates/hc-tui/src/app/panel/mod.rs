//! The studio panel's App wiring: geometry (`chrome_layout`), the
//! `prefix+key` toggle, focused keys, mouse, the action runner and the
//! working-mark animation deadline. The row model and renderer live in
//! `crate::panel`; everything here only feeds them `Panel`, the view's
//! `backlot` document and the inventory's stage flag.

mod actions;
mod input;
mod keys;
mod layout;
mod mouse;
mod select;
pub(crate) mod sheet;

use std::time::{Duration, Instant};

use hc_proto::endpoint::Endpoint;

use super::App;
use crate::machine_versions::Backlot;
use crate::panel::data::MachineSummary;
use crate::panel::{self, PanelConfig, PanelMode, Target};

/// The working-mark spin interval (~8 fps); the deadline is a one-shot
/// that re-arms only while a working mark is drawn.
pub(crate) const PANEL_SPIN: Duration = Duration::from_millis(120);

/// The collapsed strip's fixed width.
pub(crate) const COLLAPSED_WIDTH: u16 = 16;
/// An expanded dock beside the pane must leave this many columns for
/// the pane surface (S2 §1). The collapsed strip's own rule follows
/// from it: 16 + 44 = 60.
pub(crate) const MIN_PANE_BESIDE_DOCK: u16 = 44;
/// The collapsed strip sits beside the panes from this width; under
/// it the strip is a 16-column overlay at the surface's right edge.
pub(crate) const STRIP_BESIDE_COLS: u16 = COLLAPSED_WIDTH + MIN_PANE_BESIDE_DOCK;

impl App {
    // ---------- state ----------

    /// Panel keys/mouse are owned by the panel — focused beside the
    /// panes, or whenever the panel covers the frame: nobody can see
    /// the panes then, so their keys must not reach them either.
    pub(crate) fn panel_focused(&self) -> bool {
        self.panel_mode_at(self.area.0) != PanelMode::Closed
            && (self.panel.focused || self.panel_fullscreen())
    }

    /// Every machine's `backlot` summary for this view — this core's
    /// first, then each added machine's, in `machine.list` order.
    pub(crate) fn panel_summaries(&self) -> Vec<MachineSummary> {
        panel::data::summaries(&self.view)
    }

    /// Whether the `›` line owns the keys — `Compose`: the panel is
    /// focused, the selection sits on a `Input` row and the explicit
    /// `panel.editing` latch is set (R3-B2). `Resting` on the same
    /// row keeps `j`/`k`/`m`/`Esc` navigation live.
    pub(crate) fn panel_editing(&self) -> bool {
        self.panel.focused
            && self.panel.editing
            && matches!(self.panel.selected, Some(Target::Input(_)))
    }

    /// Whether `machine`'s (`None` is this core) inventory shows a
    /// live stage terminal.
    pub(super) fn machine_stage_running(&self, machine: Option<&str>) -> bool {
        panel::stage_running(&self.view, machine)
    }

    /// Whether `machine` is connected — `None` (this core) always
    /// is; an unlisted machine that still published a document reads
    /// as up so its block does not lie about reachability.
    pub(crate) fn machine_reachable(&self, machine: Option<&str>) -> bool {
        match machine {
            None => true,
            Some(name) => self
                .view
                .machines
                .as_deref()
                .unwrap_or_default()
                .iter()
                .find(|m| m.name == name)
                .map(|m| m.state == "connected")
                .unwrap_or(true),
        }
    }

    /// S2: the newest install that outranks this client — the local
    /// stage's published `backlot_version` (and build id) or an
    /// installed `backlot --version` the six-hour probe answered.
    /// `None` while nothing is newer, so the lot says nothing.
    pub(crate) fn update_hint(&self) -> Option<crate::panel::UpdateHint> {
        let mut candidates = Vec::new();
        let summaries = self.panel_summaries();
        if let Some(local) = summaries.iter().find(|summary| summary.machine.is_none())
            && let Some(version) = &local.summary.backlot_version
        {
            candidates.push((version.clone(), local.summary.backlot_build.clone()));
        }
        for machine in self.view.machines.as_deref().unwrap_or_default() {
            if let Backlot::Installed { version, .. } =
                self.machine_versions.state(&machine.name, None)
            {
                candidates.push((version, None));
            }
        }
        crate::panel::newest(
            env!("CARGO_PKG_VERSION"),
            option_env!("BACKLOT_BUILD_ID"),
            &candidates,
        )
    }

    /// The toggle chord (`prefix+<panel.key>`): closed or auto-hidden
    /// opens expanded and focused; visible-but-unfocused focuses;
    /// focused (or overlay) closes. Opening or focusing selects the
    /// first actionable row — a row cursor is always visible while the
    /// panel owns the keys (A2.2).
    pub(crate) fn panel_toggle(&mut self) {
        if self.panel_mode_at(self.area.0) == PanelMode::Closed {
            self.panel.mode = PanelMode::Expanded;
            self.panel.focused = true;
            self.panel.selected = None;
            self.panel.selected_row = None;
            self.panel.scroll = 0;
            self.panel_select_first();
            return;
        }
        // Full-screen owns the keys: the chord closes on the first
        // press, even when the focus latch is off after a resize from
        // beside to full-screen.
        if self.panel_fullscreen() || self.panel.focused {
            self.panel_close();
            return;
        }
        self.panel.focused = true;
        if self.panel.selected.is_none() {
            self.panel_select_first();
        }
    }

    /// Whether `key` is the configured `focus_lot` binding (`alt+l`
    /// by default): active in studio mode (`!plain`), disabled inside
    /// a running dock compose or inline finish confirm.
    pub(crate) fn focus_lot_press(&self, key: &super::input::TerminalKey) -> bool {
        if !self.studio.active || self.panel_editing() || self.panel.finish_confirm.is_some() {
            return false;
        }
        let matched = crate::input::terminal_key_of(key);
        self.panel_config.keys.focus_lot_matches(&matched)
    }

    /// Show and focus the lot with the selection on its last
    /// selectable row (`Alt+L`): never toggles closed and never starts
    /// composing.
    pub(crate) fn focus_lot(&mut self) {
        self.panel.mode = PanelMode::Expanded;
        self.panel.focused = true;
        self.panel_select_last();
    }

    /// Close the panel and drop every piece of interaction state.
    pub(super) fn panel_close(&mut self) {
        self.panel.mode = PanelMode::Closed;
        self.panel.focused = false;
        self.panel.editing = false;
        self.panel.reset_history_nav();
        self.panel.selected = None;
        self.panel.selected_row = None;
        self.panel.drawer = None;
        self.panel_anim_deadline = None;
    }

    /// Leave the panel after a row focused a terminal: full-screen
    /// closes (the pane becomes visible), beside stays open unfocused.
    pub(super) fn panel_leave(&mut self) {
        self.panel.editing = false;
        self.panel.reset_history_nav();
        if self.panel_fullscreen() {
            self.panel_close();
        } else {
            self.panel.focused = false;
        }
    }

    /// A press the focused pane refused while the dock sits drawn
    /// beside the panes, unfocused: the press stays refused — it is
    /// never re-read as a dock action — but the dock takes the keys
    /// from here on. One-way: a pane that comes back never takes the
    /// keys back by itself; the dock keeps them until `Esc`, whose
    /// fate `panel_esc_closes` computes from the pane's state right
    /// then. The hand-off also selects the first actionable row, so
    /// the footer's `m`/`⏎` promise holds on a dock never toggled,
    /// and the notice says where the keys went.
    pub(super) fn dead_pane_handoff(&mut self) {
        if self.panel_mode_at(self.area.0) == PanelMode::Closed
            || self.panel_fullscreen()
            || self.panel.focused
            || !self.pane.input_refused()
        {
            return;
        }
        self.panel.focused = true;
        // A dock never toggled has no row cursor — the same
        // first-row selection `panel_toggle` does when it first
        // shows the dock.
        if self.panel.selected.is_none() {
            self.panel_select_first();
        }
        // The dock hand-off says it better than the pane's generic
        // text — take the rejection so the wake does not re-show it.
        self.pane.take_input_rejection();
        self.show_notice("Pane exited", "keys go to the dock");
    }

    /// Collapsed ↔ expanded (the `c` key and the title click).
    pub(super) fn panel_flip_mode(&mut self) {
        self.panel.mode = match self.panel.mode {
            PanelMode::Collapsed => PanelMode::Expanded,
            _ => PanelMode::Collapsed,
        };
    }

    // ---------- animation ----------

    /// Whether any spinner animation deadline is currently armed.
    #[cfg(test)]
    pub(crate) fn has_animation(&self) -> bool {
        self.panel_anim_deadline.is_some() || self.opening_anim_deadline.is_some()
    }

    /// The one-shot spin deadline fired: advance the frame, redraw —
    /// the draw re-arms the deadline only while a working mark is
    /// still on screen.
    pub(crate) fn tick_panel(&mut self, now: Instant) {
        if self.panel_anim_deadline.is_some_and(|due| due <= now) {
            self.panel_anim_deadline = None;
            self.panel.spin = self.panel.spin.wrapping_add(1);
            self.sched.mark_dirty(now);
        }
    }

    // ---------- wiring ----------

    /// cli injects the resolved `[panel]` config and, when the file
    /// was malformed or carried an invalid key, its notice.
    pub(crate) fn set_panel_config(
        &mut self,
        config: PanelConfig,
        notice: Option<String>,
        path: &std::path::Path,
    ) {
        // `load` already falls back to `y` on a bad key — guard any
        // other caller the same way so the chord can never die.
        self.panel_key_combo = crate::config::parse_key_combo(&config.key)
            .or_else(|| crate::config::parse_key_combo("y"));
        self.panel_config = config;
        self.panel_config_path = path.to_path_buf();
        if let Some(notice) = notice {
            self.show_notice("Panel config", &notice);
        }
    }

    /// cli injects the endpoint's action flags so spawned `backlot`
    /// commands reach the same core's ledger.
    pub(crate) fn set_panel_endpoint(&mut self, endpoint: &Endpoint) {
        self.panel_endpoint_args = match endpoint {
            Endpoint::Local(local) => vec!["--session".into(), local.session.clone()],
            Endpoint::Remote(remote) => vec![
                "--remote".into(),
                remote.target.clone(),
                "--session".into(),
                remote.session.clone(),
            ],
        };
    }

    /// cli installs the shared-RPC runner once the client is up.
    pub(crate) fn set_remote_runner(&mut self, runner: panel::RemoteRunner) {
        self.remote_runner = Some(runner);
    }
}

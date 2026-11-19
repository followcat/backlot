//! The studio panel's App wiring: geometry (`chrome_layout`), the
//! `prefix+key` toggle, focused keys, mouse, the action runner and the
//! working-mark animation deadline. The row model and renderer live in
//! `crate::panel`; everything here only feeds them `Panel`, the view's
//! `backlot` document and the inventory's stage flag.

mod actions;
mod keys;
mod layout;
mod mouse;
mod select;
pub(crate) mod sheet;

use std::time::{Duration, Instant};

use hc_proto::endpoint::Endpoint;

use super::App;
use crate::panel::data::MachineSummary;
use crate::panel::{self, PanelConfig, PanelMode};

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
        self.panel.open() && (self.panel.focused || self.panel_fullscreen())
    }

    /// Every machine's `backlot` summary for this view — this core's
    /// first, then each added machine's, in `machine.list` order.
    pub(crate) fn panel_summaries(&self) -> Vec<MachineSummary> {
        panel::data::summaries(&self.view)
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

    /// The toggle chord (`prefix+<panel.key>`): closed opens expanded
    /// and focused; open-but-unfocused focuses; focused closes.
    /// Opening or focusing selects the first actionable row — a row
    /// cursor is always visible while the panel owns the keys (A2.2).
    pub(crate) fn panel_toggle(&mut self) {
        if !self.panel.open() {
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

    /// Close the panel and drop every piece of interaction state.
    pub(super) fn panel_close(&mut self) {
        self.panel.mode = PanelMode::Closed;
        self.panel.focused = false;
        self.panel.selected = None;
        self.panel.selected_row = None;
        self.panel.closed_hint = None;
        self.panel.drawer = None;
        self.panel_anim_deadline = None;
    }

    /// Leave the panel after a row focused a terminal: full-screen
    /// closes (the pane becomes visible), beside stays open unfocused.
    pub(super) fn panel_leave(&mut self) {
        if self.panel_fullscreen() {
            self.panel_close();
        } else {
            self.panel.focused = false;
        }
    }

    /// Collapsed ↔ expanded (the `c` key and the title click).
    pub(super) fn panel_flip_mode(&mut self) {
        self.panel.mode = match self.panel.mode {
            PanelMode::Collapsed => PanelMode::Expanded,
            _ => PanelMode::Collapsed,
        };
    }

    // ---------- animation ----------

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

    /// Tests inject a recorder; production spawns `current_exe()`.
    pub(crate) fn set_panel_runner(&mut self, runner: panel::PanelRunner) {
        self.panel_runner = runner;
    }

    /// cli installs the shared-RPC runner once the client is up.
    pub(crate) fn set_remote_runner(&mut self, runner: panel::RemoteRunner) {
        self.remote_runner = Some(runner);
    }
}

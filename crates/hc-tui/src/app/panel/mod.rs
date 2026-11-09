//! The studio panel's App wiring: geometry (`chrome_layout`), the
//! `prefix+key` toggle, focused keys, mouse, the action runner and the
//! working-mark animation deadline. The row model and renderer live in
//! `crate::panel`; everything here only feeds them `Panel`, the view's
//! `backlot` document and the inventory's stage flag.

mod keys;
mod mouse;

use std::time::{Duration, Instant};

use backlot_model::summary::Summary;
use hc_proto::endpoint::Endpoint;

use super::App;
use crate::chrome::scroll;
use crate::panel::{self, PanelConfig, PanelMode, Target};

/// The working-mark spin interval (~8 fps); the deadline is a one-shot
/// that re-arms only while a working mark is drawn.
pub(crate) const PANEL_SPIN: Duration = Duration::from_millis(120);

/// The collapsed strip's fixed width.
const COLLAPSED_WIDTH: u16 = 16;
/// A beside-panel must leave this many columns for the panes.
const MIN_PANE_COLS: u16 = 60;

impl App {
    // ---------- geometry ----------

    /// The panel's reserved width: 0 closed, 16 collapsed, the
    /// configured width expanded.
    pub(crate) fn panel_width(&self) -> u16 {
        match self.panel.mode {
            PanelMode::Closed => 0,
            PanelMode::Collapsed => COLLAPSED_WIDTH,
            PanelMode::Expanded => self
                .panel_config
                .width
                .clamp(panel::WIDTH_MIN, panel::WIDTH_MAX),
        }
    }

    /// Whether the panel draws beside the panes at `cols` — only past
    /// the mobile threshold with 60+ columns left for the panes.
    pub(crate) fn panel_beside(&self, cols: u16) -> bool {
        let width = self.panel_width();
        width > 0
            && cols > self.shell.mobile_width_threshold
            && cols.saturating_sub(width) >= MIN_PANE_COLS
    }

    /// An open panel that does not fit beside the panes draws over the
    /// whole frame; the pane geometry keeps the panel-closed shape.
    pub(crate) fn panel_fullscreen(&self) -> bool {
        self.panel.open() && !self.panel_beside(self.area.0)
    }

    /// The one geometry helper every consumer shares: pane layout
    /// with the beside-panel's columns subtracted. Closed or
    /// full-screen reserves nothing, so the pane region is untouched.
    pub(crate) fn chrome_layout(
        &self,
        cols: u16,
        rows: u16,
    ) -> crate::chrome::snapshot::ClientShellLayout {
        let reserve = u16::from(self.panel_beside(cols)) * self.panel_width();
        let mut config = self.shell.layout_config();
        if reserve > 0 {
            // The mobile/desktop decision belongs to the whole window.
            // Beside already means `cols > mobile_width_threshold`, so
            // the leftover columns must never flip the chrome to the
            // mobile layout just because they dip under it.
            config.mobile_width_threshold = 0;
        }
        config.layout(
            cols.saturating_sub(reserve),
            rows,
            self.shell_state.sidebar_collapsed,
            super::pane_geometry::focused_tab_count(self),
            self.sidebar_width,
        )
    }

    // ---------- state ----------

    /// Panel keys/mouse are owned by the panel — focused beside the
    /// panes, or whenever the panel covers the frame: nobody can see
    /// the panes then, so their keys must not reach them either.
    pub(crate) fn panel_focused(&self) -> bool {
        self.panel.open() && (self.panel.focused || self.panel_fullscreen())
    }

    /// The current `backlot` summary for this view.
    pub(crate) fn panel_summary(&self) -> Option<Summary> {
        panel::summary(&self.view)
    }

    /// The row targets as they would draw now — selection works on
    /// stable identities, never positions.
    fn panel_targets(&self) -> Vec<Target> {
        let summary = self.panel_summary();
        panel::rows(
            summary.as_ref(),
            panel::stage_running(&self.view),
            &self.panel,
        )
        .iter()
        .map(|r| r.target())
        .collect()
    }

    /// The selected row's index in `targets` — a vanished target
    /// reverts to the title (index 0).
    fn panel_selected_index(&self, targets: &[Target]) -> usize {
        self.panel
            .selected
            .as_ref()
            .and_then(|t| targets.iter().position(|r| r == t))
            .unwrap_or(0)
    }

    /// The toggle chord (`prefix+<panel.key>`): closed opens expanded
    /// and focused; open-but-unfocused focuses; focused closes.
    pub(crate) fn panel_toggle(&mut self) {
        if !self.panel.open() {
            self.panel.mode = PanelMode::Expanded;
            self.panel.focused = true;
            self.panel.selected = None;
            self.panel.reopen_confirm = None;
            self.panel.scroll = 0;
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
    }

    /// Close the panel and drop every piece of interaction state.
    pub(super) fn panel_close(&mut self) {
        self.panel.mode = PanelMode::Closed;
        self.panel.focused = false;
        self.panel.selected = None;
        self.panel.reopen_confirm = None;
        self.panel_anim_deadline = None;
    }

    /// Leave the panel after a row focused a terminal: full-screen
    /// closes (the pane becomes visible), beside stays open unfocused.
    fn panel_leave(&mut self) {
        if self.panel_fullscreen() {
            self.panel_close();
        } else {
            self.panel.focused = false;
        }
    }

    /// Collapsed ↔ expanded (the `c` key and the title click).
    fn panel_flip_mode(&mut self) {
        self.panel.mode = match self.panel.mode {
            PanelMode::Collapsed => PanelMode::Expanded,
            _ => PanelMode::Collapsed,
        };
    }

    /// j/k/arrows and paging: move the selection by `delta` rows and
    /// keep it visible. Moving clears an armed reopen.
    fn panel_move(&mut self, delta: isize) {
        let summary = self.panel_summary();
        let rows = panel::rows(
            summary.as_ref(),
            panel::stage_running(&self.view),
            &self.panel,
        );
        if rows.len() <= 1 {
            return;
        }
        let targets: Vec<Target> = rows.iter().map(|r| r.target()).collect();
        let current = self.panel_selected_index(&targets) as isize;
        let next = current
            .saturating_add(delta)
            .clamp(0, targets.len().saturating_sub(1) as isize) as usize;
        self.panel.selected = Some(targets[next].clone());
        self.panel.reopen_confirm = None;
        // Keep the selection inside the scrolled body (title is fixed).
        let body_height = self.panel_hits.body.height.max(1);
        let heights: Vec<u16> = rows.iter().skip(1).map(|r| r.height()).collect();
        self.panel.scroll = scroll::list_scroll_start_to_reveal(
            &heights,
            &[],
            body_height,
            self.panel.scroll,
            next.saturating_sub(1),
        );
    }

    /// Direct scroll (wheel/drag): write `scroll`, then clamp the
    /// selection into the shown body so rendering does not snap back.
    fn panel_scroll_to(&mut self, start: usize, viewport_rows: usize) {
        let targets = self.panel_targets();
        let viewport_rows = viewport_rows.max(1);
        let body_len = targets.len().saturating_sub(1);
        self.panel.scroll = start.min(body_len.saturating_sub(viewport_rows));
        let selected = self
            .panel_selected_index(&targets)
            .max(1)
            .clamp(self.panel.scroll + 1, self.panel.scroll + viewport_rows)
            .min(targets.len().saturating_sub(1).max(1));
        self.panel.selected = targets.get(selected).cloned();
    }

    /// Space — collapse the selected group header (only a group row
    /// carries a group; person rows use Enter for their own action).
    fn panel_collapse_selected(&mut self) {
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        let Some(Target::Group(studio, group)) = targets.get(index).cloned() else {
            return;
        };
        let key = (studio, group);
        if !self.panel.collapsed_groups.remove(&key) {
            self.panel.collapsed_groups.insert(key);
        }
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
    pub(crate) fn set_panel_config(&mut self, config: PanelConfig, notice: Option<String>) {
        // `load` already falls back to `y` on a bad key — guard any
        // other caller the same way so the chord can never die.
        self.panel_key_combo = crate::config::parse_key_combo(&config.key)
            .or_else(|| crate::config::parse_key_combo("y"));
        self.panel_config = config;
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
}

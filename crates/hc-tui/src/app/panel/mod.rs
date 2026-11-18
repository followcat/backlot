//! The studio panel's App wiring: geometry (`chrome_layout`), the
//! `prefix+key` toggle, focused keys, mouse, the action runner and the
//! working-mark animation deadline. The row model and renderer live in
//! `crate::panel`; everything here only feeds them `Panel`, the view's
//! `backlot` document and the inventory's stage flag.

mod keys;
mod mouse;
pub(crate) mod sheet;

use std::time::{Duration, Instant};

use hc_proto::endpoint::Endpoint;
use ratatui::layout::Rect;

use super::App;
use crate::chrome::scroll;
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
    // ---------- geometry ----------

    /// The panel's reserved width at `cols`: 0 closed, 16 collapsed,
    /// the dock width expanded — the `[dock]`/`[panel]` width when
    /// the file set one, else the spec's tier (38 at 140+, 36 at
    /// 100+, 28 under).
    pub(crate) fn panel_width(&self, cols: u16) -> u16 {
        match self.panel.mode {
            PanelMode::Closed => 0,
            PanelMode::Collapsed => COLLAPSED_WIDTH,
            PanelMode::Expanded if self.panel_config.width_explicit => self
                .panel_config
                .width
                .clamp(panel::WIDTH_MIN, panel::WIDTH_MAX),
            PanelMode::Expanded => match cols {
                0..=99 => 28,
                100..=139 => 36,
                _ => 38,
            },
        }
    }

    /// The columns the sidebar draws in at `cols` — the same math
    /// `layout()` applies, needed here because the beside test looks
    /// past the sidebar at the pane surface that would remain.
    fn sidebar_render_width(&self, cols: u16) -> u16 {
        if cols <= self.shell.mobile_width_threshold {
            return 0;
        }
        let config = self.shell.layout_config();
        let width = if self.shell_state.sidebar_collapsed {
            match config.sidebar_collapsed_mode {
                crate::config::SidebarCollapsedModeConfig::Compact => 4,
                crate::config::SidebarCollapsedModeConfig::Hidden => 0,
            }
        } else {
            let (min, max) = crate::config::validated_sidebar_bounds(
                config.sidebar_min_width,
                config.sidebar_max_width,
            )
            .unwrap_or((18, 36));
            self.sidebar_width.clamp(min, max)
        };
        width.min(cols.saturating_sub(1))
    }

    /// Whether the panel draws beside the panes at `cols` — the
    /// collapsed strip from `STRIP_BESIDE_COLS`, the expanded dock
    /// while the sidebar plus the dock still leave
    /// `MIN_PANE_BESIDE_DOCK` columns for the panes.
    pub(crate) fn panel_beside(&self, cols: u16) -> bool {
        match self.panel.mode {
            PanelMode::Closed => false,
            PanelMode::Collapsed => cols >= STRIP_BESIDE_COLS,
            PanelMode::Expanded => {
                cols > self.shell.mobile_width_threshold
                    && cols
                        .saturating_sub(self.sidebar_render_width(cols))
                        .saturating_sub(self.panel_width(cols))
                        >= MIN_PANE_BESIDE_DOCK
            }
        }
    }

    /// An open panel that does not fit beside the panes at `cols`
    /// draws over the pane surface — the pane geometry keeps the
    /// panel-closed shape. `panel_fullscreen` answers for the app's
    /// live size; the draw path asks for the frame it is painting.
    pub(crate) fn panel_fullscreen_at(&self, cols: u16) -> bool {
        self.panel.open() && !self.panel_beside(cols)
    }

    /// `panel_fullscreen_at` for `self.area` — key/mouse logic.
    pub(crate) fn panel_fullscreen(&self) -> bool {
        self.panel_fullscreen_at(self.area.0)
    }

    /// The rect an open-not-beside panel owns inside `pane` at
    /// `cols`: on mobile the whole surface; otherwise the surface's
    /// right `panel_width` columns — the sidebar is never covered.
    pub(crate) fn panel_overlay_rect(&self, pane: Rect, cols: u16) -> Rect {
        let width = match self.panel.mode {
            PanelMode::Closed => 0,
            PanelMode::Collapsed => COLLAPSED_WIDTH.min(pane.width),
            PanelMode::Expanded if cols <= self.shell.mobile_width_threshold => pane.width,
            PanelMode::Expanded => self.panel_width(cols).min(pane.width),
        };
        Rect::new(pane.x + pane.width - width, pane.y, width, pane.height)
    }

    /// The columns a beside dock reserves at `cols` — what modals
    /// subtract on the right so their frame never crosses the dock's
    /// border. An overlay dock reserves nothing (the modal covers it).
    pub(crate) fn dock_reserve(&self, cols: u16) -> u16 {
        if self.panel_beside(cols) {
            self.panel_width(cols)
        } else {
            0
        }
    }

    /// The horizontal band a full-buffer modal centers in: `area`
    /// minus a beside dock's columns, sidebar included — the one
    /// geometry every dimmed dialog shares (A6.2). With no dock open
    /// this is the whole frame.
    pub(crate) fn modal_band(&self, area: Rect) -> Rect {
        Rect::new(
            area.x,
            area.y,
            area.width - self.dock_reserve(area.width),
            area.height,
        )
    }

    /// The one geometry helper every consumer shares: pane layout
    /// with the beside-panel's columns subtracted. Closed or
    /// full-screen reserves nothing, so the pane region is untouched.
    pub(crate) fn chrome_layout(
        &self,
        cols: u16,
        rows: u16,
    ) -> crate::chrome::snapshot::ClientShellLayout {
        let reserve = u16::from(self.panel_beside(cols)) * self.panel_width(cols);
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
            self.studio.lot.pill || self.studio.lot.header.is_some(),
        )
    }

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
    fn machine_stage_running(&self, machine: Option<&str>) -> bool {
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

    /// The row targets as they would draw now — selection works on
    /// stable identities, never positions. The stacking decision
    /// needs the drawn body's height; pre-first-draw it is zero, and
    /// `rows` reads that as "no constraint" so every block opens.
    fn panel_targets(&self) -> Vec<Target> {
        let machines = self.panel_summaries();
        panel::rows(
            &machines,
            |m| self.machine_stage_running(m),
            |m| self.machine_reachable(m),
            &self.panel,
            self.studio.current.as_ref(),
            self.panel_hits.body.height,
        )
        .iter()
        .map(|r| r.target())
        .collect()
    }

    /// The selected row's index in `targets` — a vanished target
    /// reverts to the title (index 0). A `Studio`/`StageDown` target
    /// names several rows, so `selected_row` pins the occurrence the
    /// selection actually sits on; when rows shifted since (a publish
    /// above it), the nearest surviving occurrence keeps the place.
    fn panel_selected_index(&self, targets: &[Target]) -> usize {
        let Some(target) = self.panel.selected.as_ref() else {
            return 0;
        };
        if let Some(at) = self.panel.selected_row
            && targets.get(at) == Some(target)
        {
            return at;
        }
        targets
            .iter()
            .enumerate()
            .filter(|(_, t)| *t == target)
            .map(|(i, _)| i)
            .min_by_key(|i| i.abs_diff(self.panel.selected_row.unwrap_or(0)))
            .unwrap_or(0)
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

    /// The first actionable row's target — `j`/`k` never park on a
    /// `None` row, so the cursor starts on something real.
    fn panel_select_first(&mut self) {
        let targets = self.panel_targets();
        if let Some(index) = targets.iter().position(|t| *t != Target::None) {
            self.panel.selected = Some(targets[index].clone());
            self.panel.selected_row = Some(index);
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
    /// keep it visible. A block's frame rows share one `Studio`
    /// target — a move always lands on a different target, so the
    /// selection visibly steps. Moving clears an armed reopen.
    fn panel_move(&mut self, delta: isize) {
        let machines = self.panel_summaries();
        let rows = panel::rows(
            &machines,
            |m| self.machine_stage_running(m),
            |m| self.machine_reachable(m),
            &self.panel,
            self.studio.current.as_ref(),
            self.panel_hits.body.height,
        );
        if rows.len() <= 1 {
            return;
        }
        let targets: Vec<Target> = rows.iter().map(|r| r.target()).collect();
        let mut index = self.panel_selected_index(&targets);
        for _ in 0..delta.unsigned_abs() {
            // Skip `None` rows — titles, dividers, block bottoms and
            // spacers are never a cursor stop (A2.2).
            let next = if delta > 0 {
                (index + 1..targets.len())
                    .find(|&i| targets[i] != targets[index] && targets[i] != Target::None)
            } else {
                (0..index)
                    .rev()
                    .find(|&i| targets[i] != targets[index] && targets[i] != Target::None)
            };
            match next {
                Some(next) => index = next,
                None => break,
            }
        }
        // A person's extra lines share its target — the cursor lands
        // on the person's own row, the target's first contiguous row.
        while index > 0 && targets[index - 1] == targets[index] {
            index -= 1;
        }
        self.panel.selected = Some(targets[index].clone());
        self.panel.selected_row = Some(index);
        self.panel.closed_hint = None;
        self.panel.finish_confirm = None;
        // Keep the selection inside the scrolled body (title is fixed).
        let body_height = self.panel_hits.body.height.max(1);
        let heights: Vec<u16> = rows.iter().skip(1).map(|r| r.height()).collect();
        self.panel.scroll = scroll::list_scroll_start_to_reveal(
            &heights,
            &[],
            body_height,
            self.panel.scroll,
            index.saturating_sub(1),
        );
    }

    /// Direct scroll (wheel/drag): write `scroll`, then clamp the
    /// selection into the shown body so rendering does not snap back.
    fn panel_scroll_to(&mut self, start: usize, viewport_rows: usize) {
        let targets = self.panel_targets();
        let viewport_rows = viewport_rows.max(1);
        // `scroll` is a line offset and rows are variable height: the
        // bound is the drawn metrics' line maximum, not a row count —
        // otherwise the wheel dies above the bottom on two-line rows.
        let max = self
            .panel_hits
            .metrics
            .map(|m| m.max_offset_from_bottom)
            .unwrap_or_else(|| targets.len().saturating_sub(viewport_rows));
        self.panel.scroll = start.min(max);
        let lo = (self.panel.scroll + 1).min(targets.len());
        let hi = (self.panel.scroll + viewport_rows)
            .min(targets.len().saturating_sub(1))
            .max(lo);
        let mut selected = self.panel_selected_index(&targets).clamp(lo, hi);
        // The clamp can land on a `None` row — walk to the nearest
        // actionable one inside the viewport instead.
        if targets.get(selected) == Some(&Target::None) {
            selected = (selected..=hi)
                .find(|&i| targets[i] != Target::None)
                .or_else(|| (lo..selected).rev().find(|&i| targets[i] != Target::None))
                .unwrap_or(selected);
        }
        self.panel.selected = targets
            .get(selected)
            .filter(|t| **t != Target::None)
            .cloned();
        self.panel.selected_row = self.panel.selected.as_ref().map(|_| selected);
    }

    /// Space — the S2 meaning by row: a studio header folds or
    /// unfolds its block, a role or task row toggles its three-line
    /// drawer, the `✓ <n> accepted` line opens or shuts the fold.
    /// A stacked `╶` line and a strip row still fold/unfold their
    /// studio; anything else does nothing.
    fn panel_collapse_selected(&mut self) {
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        let target = targets.get(index).cloned();
        match target {
            // A role/task row's drawer — `drawer` names one owner at
            // a time, so a second Space on another row moves it.
            Some(Target::Person(..) | Target::Task(..)) => {
                let target = target.unwrap();
                if self.panel.drawer.as_ref() == Some(&target) {
                    self.panel.drawer = None;
                } else {
                    self.panel.drawer = Some(target);
                }
            }
            Some(Target::Fold(studio)) => {
                if !self.panel.accepted_folds.remove(&studio) {
                    self.panel.accepted_folds.insert(studio);
                }
            }
            // The collapsed strip has no blocks — Space on a strip
            // row opens the dock like Enter does.
            Some(Target::Strip(_)) => self.panel_flip_mode(),
            Some(studio @ (Target::Studio(_) | Target::Block(_))) => {
                let id = studio.studio().unwrap().clone();
                self.panel_block_toggle(id);
            }
            _ => {}
        }
    }

    /// Fold or unfold one studio block by hand — `expanded_set`
    /// resolves the shape; this writes the opposite manual override.
    fn panel_block_toggle(&mut self, studio: crate::studio::StudioKey) {
        let machines = self.panel_summaries();
        let rows = panel::rows(
            &machines,
            |m| self.machine_stage_running(m),
            |m| self.machine_reachable(m),
            &self.panel,
            self.studio.current.as_ref(),
            self.panel_hits.body.height,
        );
        let open = rows
            .iter()
            .any(|r| matches!(r, panel::Row::BlockTop { studio: s, .. } if s.key() == studio));
        if open {
            self.panel.expanded_blocks.remove(&studio);
            self.panel.collapsed_blocks.insert(studio.clone());
            // The block's identity changes with its shape — ride the
            // `╶` line so the next Space toggles it back open.
            self.panel.selected = Some(Target::Block(studio.clone()));
            self.panel.selected_row = self
                .panel_targets()
                .iter()
                .position(|t| matches!(t, Target::Block(s) if *s == studio));
        } else {
            self.panel.collapsed_blocks.remove(&studio);
            self.panel.expanded_blocks.insert(studio.clone());
            self.panel.selected = Some(Target::Studio(studio.clone()));
            self.panel.selected_row = self
                .panel_targets()
                .iter()
                .position(|t| *t == Target::Studio(studio.clone()));
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

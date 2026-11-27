//! The dock's geometry answers: how wide it is at each mode and
//! window size, whether it sits beside the pane or overlays it, the
//! band every modal draws inside and the pane-surface layout that
//! reserves it.

use ratatui::layout::Rect;

use super::App;
use super::{COLLAPSED_WIDTH, MIN_PANE_BESIDE_DOCK, STRIP_BESIDE_COLS};
use crate::app::Mode;
use crate::panel::{self, PanelMode};

/// Minimum terminal width where the dock may sit beside the panes
/// while unfocused (`R3-1` / `R3-M3`). Below this, unfocused docks
/// auto-hide to 0 columns and `Ctrl+B y` opens an Expanded overlay.
const BESIDE_MIN_COLS: u16 = 100;

impl App {
    /// The mode the dock draws at `cols`: the chosen mode, except an
    /// unfocused dock below `BESIDE_MIN_COLS` (or when it cannot fit
    /// beside the panes with `MIN_PANE_BESIDE_DOCK` columns left)
    /// auto-hides to `Closed` (0 columns). When focused, `Expanded`
    /// stays `Expanded` and draws as a `pane_surface` overlay (`R3-B4`).
    /// `panel.mode` keeps the user's choice, so widening restores it.
    pub(crate) fn panel_mode_at(&self, cols: u16) -> PanelMode {
        match self.panel.mode {
            PanelMode::Closed => PanelMode::Closed,
            PanelMode::Collapsed => {
                if !self.panel.focused && !self.strip_beside(cols) {
                    PanelMode::Closed
                } else {
                    PanelMode::Collapsed
                }
            }
            PanelMode::Expanded => {
                if !self.panel.focused && !self.expanded_beside(cols) {
                    PanelMode::Closed
                } else {
                    PanelMode::Expanded
                }
            }
        }
    }

    /// The expanded column's width: the `[dock]`/`[panel]` width
    /// when the file set one, else the spec's tier (38 at 140+, 36
    /// under).
    fn expanded_width(&self, cols: u16) -> u16 {
        // The border drag's manual width wins for the session —
        // over the file's width and the spec's tier (S5 §2).
        if let Some(width) = self.panel.drag_width {
            return width.clamp(panel::WIDTH_MIN, panel::WIDTH_MAX);
        }
        if self.panel_config.width_explicit {
            return self
                .panel_config
                .width
                .clamp(panel::WIDTH_MIN, panel::WIDTH_MAX);
        }
        if cols >= 140 { 38 } else { 36 }
    }

    /// The panel's reserved width at `cols`: 0 when closed or
    /// auto-hidden, 16 when collapsed, else `expanded_width(cols)`.
    pub(crate) fn panel_width(&self, cols: u16) -> u16 {
        match self.panel_mode_at(cols) {
            PanelMode::Closed => 0,
            PanelMode::Collapsed => COLLAPSED_WIDTH,
            PanelMode::Expanded => self.expanded_width(cols),
        }
    }

    /// The columns the sidebar draws in at `cols` — the same math
    /// `layout()` applies, needed here because the beside test looks
    /// past the sidebar at the pane surface that would remain.
    pub(crate) fn sidebar_render_width(&self, cols: u16) -> u16 {
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

    /// Whether the expanded column fits beside the panes at `cols`:
    /// desktop width (`>= 100` and above `mobile_width_threshold`) and
    /// sidebar plus dock still leave `MIN_PANE_BESIDE_DOCK` columns.
    fn expanded_beside(&self, cols: u16) -> bool {
        cols >= BESIDE_MIN_COLS
            && cols > self.shell.mobile_width_threshold
            && cols
                .saturating_sub(self.sidebar_render_width(cols))
                .saturating_sub(self.expanded_width(cols))
                >= MIN_PANE_BESIDE_DOCK
    }

    /// Whether the collapsed strip may sit beside the panes at `cols`:
    /// requires desktop width (`>= 100` and above
    /// `mobile_width_threshold`), `STRIP_BESIDE_COLS`, and
    /// `MIN_PANE_BESIDE_DOCK` columns past the sidebar.
    fn strip_beside(&self, cols: u16) -> bool {
        cols >= BESIDE_MIN_COLS
            && cols > self.shell.mobile_width_threshold
            && cols >= STRIP_BESIDE_COLS
            && cols
                .saturating_sub(self.sidebar_render_width(cols))
                .saturating_sub(COLLAPSED_WIDTH)
                >= MIN_PANE_BESIDE_DOCK
    }

    /// Whether the panel draws beside the panes at `cols`.
    pub(crate) fn panel_beside(&self, cols: u16) -> bool {
        match self.panel_mode_at(cols) {
            PanelMode::Closed => false,
            PanelMode::Collapsed => self.strip_beside(cols),
            PanelMode::Expanded => self.expanded_beside(cols),
        }
    }

    /// An open panel that is visible at `cols` and does not sit beside
    /// the panes draws over `pane_surface` — the pane geometry keeps the
    /// panel-closed shape. `panel_fullscreen` answers for the app's
    /// live size; the draw path asks for the frame it is painting.
    pub(crate) fn panel_fullscreen_at(&self, cols: u16) -> bool {
        self.panel_mode_at(cols) != PanelMode::Closed && !self.panel_beside(cols)
    }

    /// `panel_fullscreen_at` for `self.area` — key/mouse logic.
    pub(crate) fn panel_fullscreen(&self) -> bool {
        self.panel_fullscreen_at(self.area.0)
    }

    /// `Esc` on the focused dock closes it outright — an overlay dock
    /// has no panes beside it to hand the keys back to, and a dock
    /// over a pane that refuses input has nowhere to return them
    /// either. Beside a live pane `Esc` only steps back: the pane
    /// takes the keys again. The footer's label in `panel/draw.rs`
    /// reads the same two facts, so it cannot promise a step-back
    /// the key does not do.
    pub(crate) fn panel_esc_closes(&self) -> bool {
        self.panel_fullscreen() || self.pane.input_refused()
    }

    /// The rect an overlay panel owns inside `pane` at `cols`: on mobile
    /// (`cols <= mobile_width_threshold`), an expanded dock takes the
    /// full `pane.width`; on narrow desktop it right-aligns inside
    /// `pane` at `expanded_width(cols)` (clamped to `pane.width`), so
    /// the sidebar is never covered.
    pub(crate) fn panel_overlay_rect(&self, pane: Rect, cols: u16) -> Rect {
        let width = match self.panel_mode_at(cols) {
            PanelMode::Closed => 0,
            PanelMode::Collapsed => COLLAPSED_WIDTH.min(pane.width),
            PanelMode::Expanded => {
                if cols <= self.shell.mobile_width_threshold {
                    pane.width
                } else {
                    self.expanded_width(cols).min(pane.width)
                }
            }
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

    /// The horizontal band the expanded Call Sheet spans: everything
    /// right of the rendered sidebar (`pane + dock`), so the left
    /// sidebar (`machines` and `cast` headings included) stays fully
    /// visible (`R3-4` / `wide-13`).
    pub(crate) fn call_sheet_band(&self, area: Rect) -> Rect {
        let sidebar_w = self.sidebar_render_width(area.width);
        Rect::new(
            area.x + sidebar_w,
            area.y,
            area.width.saturating_sub(sidebar_w),
            area.height,
        )
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

    /// The centering rect for Backlot's own modals (`R3-M2`):
    /// `layout.pane_surface` when it is at least `min_modal_w` wide
    /// (so modals stay between the sidebar and any beside-Dock),
    /// falling back to `modal_band(area)` (full viewport when no
    /// beside-Dock is open, e.g. `80×24` / `60×20`).
    pub(crate) fn backlot_modal_surface(
        &self,
        area: Rect,
        layout: &crate::chrome::snapshot::ClientShellLayout,
        min_modal_w: u16,
    ) -> Rect {
        if layout.pane_surface.width >= min_modal_w {
            layout.pane_surface
        } else {
            self.modal_band(area)
        }
    }

    /// The studio-mode toast/notice anchor rect (`R3-M12`): top-right
    /// of `layout.pane_surface` (left of any beside-Dock), pushed
    /// below an expanded Call Sheet band when one covers the top of
    /// `pane_surface`.
    pub(crate) fn toast_surface(
        &self,
        _area: Rect,
        layout: &crate::chrome::snapshot::ClientShellLayout,
    ) -> Rect {
        let mut rect = layout.pane_surface;
        if self.hollywood.expanded() && self.hollywood_hits.area.height > 0 {
            let below = self.hollywood_hits.area.bottom();
            if below > rect.y {
                let bottom = rect.bottom();
                rect.y = below.min(bottom);
                rect.height = bottom.saturating_sub(rect.y);
            }
        }
        rect
    }

    /// Whether any modal (Backlot or Herdr covering Backlot surfaces)
    /// is currently open — used to suppress underlying Dock and Call
    /// Sheet active highlights, `▌` bars, and secondary cursors (`R3-M2`).
    /// Whether any dimming Backlot/Herdr modal is currently open above
    /// underlying surfaces (excludes `ContextMenu` and `Popup` which do
    /// not dim underlying highlights, matching `R3-M2`).
    pub(crate) fn has_active_modal(&self) -> bool {
        self.focus.modals.iter().any(|m| {
            !matches!(
                m,
                crate::focus::ModalLayer::ContextMenu | crate::focus::ModalLayer::Popup
            )
        }) || self.new_studio.is_some()
            || self.hw_switch.is_some()
            || self.sheet.is_some()
            || self.hollywood.view.is_some()
            || self.studio_dialog.is_some()
            || self.peek.is_some()
            || self.help
            || self.settings.is_some()
            || self.navigator.is_some()
            || self.dialog.is_some()
            || self.update_modal.is_some()
    }

    /// The focused pane's hardware cursor stays hidden while any
    /// overlay owns the frame — every modal above plus the sheet and
    /// a full-screen dock; otherwise it would print through the
    /// covering surface (S2 round-1 review).
    pub(crate) fn hide_pane_cursor(&self, cols: u16) -> bool {
        self.help
            || self.dialog.is_some()
            || self.new_studio.is_some()
            || self.hw_switch.is_some()
            || self.sheet.is_some()
            || self.studio_dialog.is_some()
            || self.hollywood.view.is_some()
            || self.peek.is_some()
            || self.menu.is_some()
            || self.navigator.is_some()
            || self.settings.is_some()
            || self.panel_fullscreen_at(cols)
            || self.opening_visible()
            || self.mode == Mode::Copy
            || (self.mode == Mode::Navigate && self.area.0 <= self.shell.mobile_width_threshold)
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
        if cols > self.shell.mobile_width_threshold && reserve > 0 {
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
            crate::app::pane_geometry::focused_tab_count(self),
            self.sidebar_width,
            self.studio.lot.pill || self.studio.lot.header.is_some(),
        )
    }
}

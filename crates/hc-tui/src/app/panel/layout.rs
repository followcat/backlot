//! The dock's geometry answers: how wide it is at each mode and
//! window size, whether it sits beside the pane or overlays it, the
//! band every modal draws inside and the pane-surface layout that
//! reserves it.

use ratatui::layout::Rect;

use super::App;
use super::{COLLAPSED_WIDTH, MIN_PANE_BESIDE_DOCK, STRIP_BESIDE_COLS};
use crate::app::Mode;
use crate::panel::{self, PanelMode};

impl App {
    /// The panel's reserved width at `cols`: 0 closed, 16 collapsed,
    /// the dock width expanded — the `[dock]`/`[panel]` width when
    /// the file set one, else the spec's tier (38 at 140+, 36 at
    /// 100+, 28 under).
    pub(crate) fn panel_width(&self, cols: u16) -> u16 {
        match self.panel.mode {
            PanelMode::Closed => 0,
            PanelMode::Collapsed => COLLAPSED_WIDTH,
            // The border drag's manual width wins for the session —
            // over the file's width and the spec's tier (S5 §2).
            PanelMode::Expanded if let Some(width) = self.panel.drag_width => {
                width.clamp(panel::WIDTH_MIN, panel::WIDTH_MAX)
            }
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
    pub(super) fn sidebar_render_width(&self, cols: u16) -> u16 {
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

    /// The focused pane's hardware cursor stays hidden while any
    /// overlay owns the frame — every modal above plus the sheet and
    /// a full-screen dock; otherwise it would print through the
    /// covering surface (S2 round-1 review).
    pub(crate) fn hide_pane_cursor(&self, cols: u16) -> bool {
        self.help
            || self.dialog.is_some()
            || self.new_studio.is_some()
            || self.sheet.is_some()
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
            crate::app::pane_geometry::focused_tab_count(self),
            self.sidebar_width,
            self.studio.lot.pill || self.studio.lot.header.is_some(),
        )
    }
}

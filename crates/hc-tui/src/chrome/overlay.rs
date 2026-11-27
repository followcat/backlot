//! Opaqueness for overlay surfaces in backlot's own look.
//!
//! A popup's terminal or a card's text may leave cells on the
//! terminal's default background. In a translucent terminal that shows
//! the window behind the popup — a wizard must not be a window. The
//! backlot theme fills those cells with the panel background; every
//! reference theme keeps its translucency exactly.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

use super::palette::Palette;

/// Whether this palette uses an opaque frame surface. Stock Herdr themes and
/// `terminal` leave `sidebar_bg == Reset`, unless backlot studio mode sets it
/// to `elevation.surface`; explicit `Reset` / `terminal` keeps translucency.
pub(crate) fn opaque(palette: &Palette) -> bool {
    palette.sidebar_bg != Color::Reset && palette.panel_bg != Color::Reset
}

/// Top bar background color: `elevation.stage_well` when studio chrome
/// is active (all non-`terminal` RGB themes under `!plain && source != "herdr"`),
/// otherwise `palette.panel_bg`.
pub(crate) fn chrome_bg(config: &super::ClientShellConfig) -> Color {
    if config.studio_chrome {
        let well = config.palette.elevation().stage_well;
        if well != Color::Reset {
            return well;
        }
    }
    config.palette.panel_bg
}

/// Fill every default-background cell in `area` with the panel
/// background. A no-op for the reference themes.
pub(crate) fn fill_default_bg(buffer: &mut Buffer, area: Rect, palette: &Palette) {
    if !opaque(palette) {
        return;
    }
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let cell = &mut buffer[(x, y)];
            if cell.bg == Color::Reset {
                cell.bg = palette.panel_bg;
            }
        }
    }
}

#[cfg(test)]
#[path = "overlay_tests.rs"]
mod tests;

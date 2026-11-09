// Derived from herdr src/client/shell/composition.rs
// `client_copy_surface_coherent`, `render_client_copy_search_highlights`
// and the Copy-mode cursor paint (Apache-2.0).
//! Copy-mode overlay rendering: search-match highlights (other matches
//! under the selection, the current match over it) and the single-cell
//! copy cursor.

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};

use super::copy_mode::CopyModeState;
use crate::chrome::palette::Palette;
use crate::chrome::render::PaneHit;

/// The reference's `client_copy_surface_coherent`: copy-mode state is
/// painted only while the surface's geometry and scroll offsets still
/// match the copy snapshot — a mid-resize frame skips the overlay.
pub(super) fn copy_surface_coherent(copy_mode: Option<&CopyModeState>, hit: &PaneHit) -> bool {
    copy_mode
        .filter(|copy_mode| copy_mode.pane_id == hit.pane_id)
        .is_none_or(|copy_mode| {
            copy_mode.geometry == (hit.inner_rect.width, hit.inner_rect.height)
                && hit.scroll.is_some_and(|scroll| {
                    scroll.offset_from_bottom == copy_mode.offset_from_bottom
                        && scroll.max_offset_from_bottom == copy_mode.max_offset_from_bottom
                })
        })
}

/// The reference's `render_client_copy_search_highlights`: non-current
/// matches take the subtle style, the current match the accent style.
pub(super) fn render_copy_search_highlights(
    buffer: &mut Buffer,
    copy_mode: Option<&CopyModeState>,
    hit: &PaneHit,
    palette: &Palette,
    current_only: bool,
) {
    let Some(copy_mode) = copy_mode.filter(|copy_mode| copy_mode.pane_id == hit.pane_id) else {
        return;
    };
    if hit.inner_rect.is_empty() {
        return;
    }
    let top = copy_mode
        .max_offset_from_bottom
        .saturating_sub(copy_mode.offset_from_bottom)
        .min(u32::MAX as usize) as u32;
    let bottom = top.saturating_add(u32::from(hit.inner_rect.height.saturating_sub(1)));
    let style = if current_only {
        Style::default()
            .fg(panel_contrast_fg(palette))
            .bg(palette.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette.text).bg(palette.surface1)
    };
    for (index, text_match) in copy_mode.search_matches.iter().enumerate() {
        if (copy_mode.search_current == Some(index)) != current_only
            || text_match.end.row < top
            || text_match.start.row > bottom
        {
            continue;
        }
        let start_row = text_match.start.row.max(top);
        let end_row = text_match.end.row.min(bottom);
        for absolute_row in start_row..=end_row {
            let viewport_row = absolute_row.saturating_sub(top) as u16;
            let start_col = if absolute_row == text_match.start.row {
                text_match.start.col
            } else {
                0
            };
            let end_col = if absolute_row == text_match.end.row {
                text_match.end.col
            } else {
                hit.inner_rect.width.saturating_sub(1)
            };
            let end_col = end_col.min(hit.inner_rect.width.saturating_sub(1));
            for col in start_col..=end_col {
                buffer[(
                    hit.inner_rect.x.saturating_add(col),
                    hit.inner_rect.y.saturating_add(viewport_row),
                )]
                    .set_style(style);
            }
        }
    }
}

/// The reference's Copy-mode cursor paint: the program cursor is hidden
/// and the copy cursor cell takes the accent style. Returns whether the
/// cursor landed inside the surface — the reference only rebuilds the
/// frame (dropping extended underline bits) when it does.
pub(super) fn render_copy_cursor(
    buffer: &mut Buffer,
    copy_mode: &CopyModeState,
    hit: &PaneHit,
    palette: &Palette,
) -> bool {
    let viewport_top = copy_mode
        .max_offset_from_bottom
        .saturating_sub(copy_mode.offset_from_bottom)
        .min(u32::MAX as usize) as u32;
    let viewport_row = copy_mode.cursor.row.saturating_sub(viewport_top);
    let x = hit.inner_rect.x.saturating_add(copy_mode.cursor.col);
    let y = hit.inner_rect.y.saturating_add(viewport_row as u16);
    if viewport_row < u32::from(hit.inner_rect.height)
        && copy_mode.cursor.col < hit.inner_rect.width
        && x < buffer.area.width
        && y < buffer.area.height
    {
        buffer[(x, y)].set_style(
            Style::default()
                .fg(match palette.panel_bg {
                    Color::Reset => palette.surface_dim,
                    color => color,
                })
                .bg(palette.accent)
                .add_modifier(Modifier::BOLD),
        );
        return true;
    }
    false
}

/// The reference's `panel_contrast_fg`.
fn panel_contrast_fg(palette: &Palette) -> Color {
    match palette.panel_bg {
        Color::Reset => palette.surface_dim,
        color => color,
    }
}

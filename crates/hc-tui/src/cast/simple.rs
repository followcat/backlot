//! The cast's one-row painters: the `shells` group header and the
//! `others` toggle. Split out of `draw.rs` to keep it under the
//! file-size guideline.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;

/// The `shells` group header — a dim label line between the people
/// and their shells.
pub(crate) fn draw_shells_header(buffer: &mut Buffer, rect: Rect, palette: &Palette) {
    put_text(
        buffer,
        rect.x,
        rect.y,
        rect.width,
        " shells",
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
}

/// The `others` row: ` others <n> ▸` collapsed, `▾` expanded — the
/// in-place expansion of herdr's own agent rows.
pub(crate) fn draw_others(
    buffer: &mut Buffer,
    rect: Rect,
    count: usize,
    expanded: bool,
    palette: &Palette,
) {
    let arrow = if expanded { "▾" } else { "▸" };
    put_text(
        buffer,
        rect.x,
        rect.y,
        rect.width,
        &format!(" others {count} {arrow}"),
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
}

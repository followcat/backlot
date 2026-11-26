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

/// Draw the subtle 1-row `┄ closed (<n>) ┄` hairline separating open
/// roles from closed roles in the Cast sidebar.
pub(crate) fn draw_closed_divider(
    buffer: &mut Buffer,
    rect: Rect,
    count: usize,
    palette: &Palette,
) {
    if rect.is_empty() {
        return;
    }
    let elev = palette.elevation();
    let label = format!(" closed ({count}) ");
    let lw = crate::chrome::text::display_width(&label);
    let w = rect.width as usize;
    if lw >= w {
        put_text(
            buffer,
            rect.x,
            rect.y,
            rect.width,
            label.trim(),
            Style::default().fg(elev.muted_surface),
        );
        return;
    }
    let left = (w - lw) / 2;
    let right = w - lw - left;
    let rule_style = Style::default().fg(elev.border_muted);
    let mut x = rect.x;
    if left > 0 {
        put_text(
            buffer,
            x,
            rect.y,
            left as u16,
            &"┄".repeat(left),
            rule_style,
        );
        x = x.saturating_add(left as u16);
    }
    put_text(
        buffer,
        x,
        rect.y,
        lw as u16,
        &label,
        Style::default().fg(elev.muted_surface),
    );
    x = x.saturating_add(lw as u16);
    if right > 0 {
        put_text(
            buffer,
            x,
            rect.y,
            right as u16,
            &"┄".repeat(right),
            rule_style,
        );
    }
}

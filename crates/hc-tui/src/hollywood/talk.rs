//! The band's conversation rows: one exchange line (Hollywood's own
//! side in accent, the person's dimmer) and the fold row that stands
//! in for the older turns. The newest Hollywood line may draw taller
//! than one row — its wrap — so its text prints across those rows.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use backlot_model::summary::Exchange;

use super::band::MARGIN;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::truncate_end;
use crate::hollywood::reader_wrap::wrap_plain;

/// The room a talk line's text has at band width `width`, after the
/// left margin and the `who › ` prefix (or Hollywood's single `✦`).
pub(crate) fn talk_text_width(width: u16, who: &str) -> usize {
    let left = if who == "hollywood" {
        1
    } else {
        who.chars().count() as u16 + 2
    };
    width.saturating_sub(MARGIN + left + 1) as usize
}

/// One exchange line: `✦ <text>` for Hollywood's half (its reported
/// headline or card, in the accent the headline row carries), the
/// sender's `who ›` for every other line — dimmer, the dialog's other
/// side. A line drawn taller than one row (the newest Hollywood line's
/// wrap) prints its text across those rows; the last row is ellipsized
/// past the width.
pub(crate) fn draw_talk(
    buf: &mut Buffer,
    rect: Rect,
    line: &Exchange,
    selected: bool,
    palette: &Palette,
) {
    let base = if selected {
        Style::default().bg(palette.active_row_bg)
    } else {
        Style::default().bg(palette.surface_dim)
    };
    buf.set_style(rect, base);
    let (marker, who_style, text_style) = if line.who == "hollywood" {
        (
            "✦".to_string(),
            base.fg(palette.accent),
            base.fg(palette.text),
        )
    } else {
        (
            format!("{} ›", line.who),
            base.fg(palette.subtext0),
            base.fg(palette.subtext0),
        )
    };
    let x = rect.x + MARGIN;
    let left = marker.chars().count() as u16;
    // The marker sits on the first row only; a wrapped continuation
    // keeps the text column.
    put_text(buf, x, rect.y, left, &marker, who_style);
    let width = rect.width.saturating_sub(MARGIN + left + 1);
    let height = rect.height.max(1);
    if height <= 1 {
        put_text(
            buf,
            x + left + 1,
            rect.y,
            width,
            &truncate_end(&line.text, width as usize),
            text_style,
        );
        return;
    }
    for (i, row) in wrap_plain(&line.text, width as usize, height as usize)
        .into_iter()
        .enumerate()
    {
        put_text(
            buf,
            x + left + 1,
            rect.y + i as u16,
            width,
            &row,
            text_style,
        );
    }
}

/// `… <n> earlier · ↵ read` — the older lines folded into one dim row.
/// Enter on it opens the newest folded entry.
pub(crate) fn draw_fold(
    buf: &mut Buffer,
    rect: Rect,
    count: usize,
    selected: bool,
    palette: &Palette,
) {
    let bg = if selected {
        palette.active_row_bg
    } else {
        palette.surface_dim
    };
    buf.set_style(rect, Style::default().bg(bg));
    let text = format!("… {count} earlier · ↵ read");
    let width = rect.width.saturating_sub(MARGIN);
    put_text(
        buf,
        rect.x + MARGIN,
        rect.y,
        width,
        &truncate_end(&text, width as usize),
        Style::default().fg(palette.overlay0).bg(bg),
    );
}

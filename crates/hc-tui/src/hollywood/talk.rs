//! The band's conversation rows: fixed speaker column, right-aligned
//! relative age (with optional `· N earlier` when unfocused/collapsed) or
//! clickable ` ↵ read ` chip when selected, and up to 2 wrapped lines.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use backlot_model::summary::Exchange;

use super::draw::HollywoodRender;
use super::{MARGIN, Target};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};
use crate::hollywood::reader_wrap::wrap_plain;

/// Right-slot width reserved for relative age (` 21m`) or ` ↵ read ` chip.
const RIGHT_SLOT_W: u16 = 8;

/// Speaker column + 2-cell gap width: 11+2 at wide (`>= 90`), 5+2 narrow.
pub(crate) fn speaker_cols(width: u16) -> u16 {
    if width >= 90 { 13 } else { 7 }
}

/// The room a talk line's body text gets at band width `width`.
pub(crate) fn talk_text_width(width: u16, _who: &str) -> usize {
    width.saturating_sub(MARGIN + speaker_cols(width) + 2 + RIGHT_SLOT_W + MARGIN) as usize
}

/// Draw one exchange row: fixed speaker column (`✦ hollywood` / `› you`
/// / `· <who>`), body wrapped up to `rect.height.clamp(1, 2)` lines, and
/// right-aligned age (plus `· <earlier> earlier` when folded inline) or
/// ` ↵ read ` chip on line 1 when selected.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_talk(
    buf: &mut Buffer,
    rect: Rect,
    line: &Exchange,
    selected: bool,
    zone_focused: bool,
    now_ms: u64,
    earlier: usize,
    hide_age: bool,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    let elev = palette.elevation();
    let bg = if selected {
        elev.raised_row
    } else {
        elev.surface
    };
    let base = Style::default().bg(bg);
    buf.set_style(rect, base);
    let rail = elev.gilt_style(elev.surface);
    for dy in 0..rect.height {
        let ry = rect.y + dy;
        if zone_focused || selected {
            put_text(buf, rect.x, ry, 1, "▌", base.fg(palette.accent));
        } else {
            put_text(buf, rect.x, ry, 1, "║", rail);
        }
        if rect.width >= 2 {
            put_text(buf, rect.x + rect.width - 1, ry, 1, "║", rail);
        }
    }
    if selected {
        put_text(buf, rect.x + 1, rect.y, 1, "▸", base.fg(palette.accent));
    }

    let wide = rect.width >= 90;
    let label_x = rect.x + MARGIN;
    let label_w = if wide { 11u16 } else { 5u16 };
    let text_style = if line.who == "hollywood" {
        base.fg(palette.text)
    } else {
        base.fg(palette.subtext0)
    };
    paint_speaker(
        buf, label_x, rect.y, label_w, wide, &line.who, base, palette,
    );

    let show_read = selected && rect.width >= 60;
    let right_edge = (rect.x + rect.width).saturating_sub(MARGIN);
    let body_x = label_x + label_w + 2;
    let body_end = if show_read {
        let chip = " ↵ read ";
        let w = display_width(chip) as u16;
        let cx = right_edge.saturating_sub(w);
        if cx > body_x + 2 {
            let chip_bg = if elev.is_dark {
                palette.surface0
            } else {
                elev.surface
            };
            let chip_style = Style::default().fg(palette.subtext0).bg(chip_bg);
            put_text(buf, cx, rect.y, w, chip, chip_style);
            hits.chips
                .push((Rect::new(cx, rect.y, w, 1), Target::Talk(line.seq), '\r'));
            cx.saturating_sub(2)
        } else {
            right_edge
        }
    } else if hide_age && earlier == 0 {
        right_edge
    } else {
        let age = if hide_age {
            None
        } else {
            super::reader_meta::age_label(line.ts_ms, now_ms)
        };
        let right_label = match (age, earlier) {
            (Some(a), 0) => Some(a),
            (Some(a), n) => Some(format!("{a} · {n} earlier")),
            (None, 0) => None,
            (None, n) => Some(format!("{n} earlier")),
        };
        if let Some(label) = right_label {
            let w = display_width(&label) as u16;
            let ax = right_edge.saturating_sub(w);
            if ax > body_x + 2 {
                put_text(buf, ax, rect.y, w, &label, base.fg(elev.muted_surface));
                right_edge
                    .saturating_sub(RIGHT_SLOT_W.max(w))
                    .saturating_sub(2)
            } else {
                right_edge
            }
        } else {
            right_edge
        }
    };
    let body_w = body_end.saturating_sub(body_x);
    if body_w == 0 {
        return;
    }
    let height = rect.height.clamp(1, 2);
    if height == 1 {
        put_text(
            buf,
            body_x,
            rect.y,
            body_w,
            &truncate_end(&line.text, body_w as usize),
            text_style,
        );
        return;
    }
    for (i, row) in wrap_plain(&line.text, body_w as usize, height as usize)
        .into_iter()
        .enumerate()
    {
        put_text(buf, body_x, rect.y + i as u16, body_w, &row, text_style);
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_speaker(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    label_w: u16,
    wide: bool,
    who: &str,
    base: Style,
    palette: &Palette,
) {
    let elev = palette.elevation();
    if who == "hollywood" {
        put_text(
            buf,
            x,
            y,
            1,
            "✦",
            base.fg(palette.accent).add_modifier(Modifier::BOLD),
        );
        let name = if wide { "hollywood" } else { "hw" };
        put_text(
            buf,
            x + 2,
            y,
            label_w.saturating_sub(2),
            name,
            base.fg(palette.accent),
        );
    } else if who == "user" {
        put_text(
            buf,
            x,
            y,
            1,
            "›",
            base.fg(palette.teal).add_modifier(Modifier::BOLD),
        );
        put_text(
            buf,
            x + 2,
            y,
            label_w.saturating_sub(2),
            "you",
            base.fg(palette.subtext0),
        );
    } else {
        put_text(buf, x, y, 1, "·", base.fg(elev.muted_surface));
        let clipped = truncate_end(who, label_w.saturating_sub(2) as usize);
        put_text(
            buf,
            x + 2,
            y,
            label_w.saturating_sub(2),
            &clipped,
            base.fg(elev.muted_surface),
        );
    }
}

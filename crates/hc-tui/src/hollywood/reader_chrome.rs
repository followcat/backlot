//! Reader frame chrome: Row 1 border (`left` + right `◀ i/N ▶ · <age>`),
//! Row 2 semantic context row, `surface1` dividers, `surface0` action
//! strip, bottom scroll range, and styled span emitter.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;

use super::reader_meta::HeaderLayout;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};

/// Draw the top border with left title/kind badge and optional right
/// nav controls (`◀ i/N ▶ · <age>`), returning `(prev_hit, next_hit)`.
pub(super) fn draw_top_border(
    buf: &mut Buffer,
    area: Rect,
    header: &HeaderLayout,
    edge: Style,
    palette: &Palette,
) -> (Option<Rect>, Option<Rect>) {
    if area.width < 2 {
        return (None, None);
    }
    if header.right.is_empty() {
        let max_title_w = (area.width as usize).saturating_sub(4);
        let left = format!("╭ {} ", truncate_end(&header.left, max_title_w));
        let left_w = display_width(&left) as u16;
        put_text(
            buf,
            area.x,
            area.y,
            left_w.min(area.width - 1),
            &left,
            edge.add_modifier(Modifier::BOLD),
        );
        let fill = (area.width - 1).saturating_sub(left_w);
        if fill > 0 {
            put_text(
                buf,
                area.x + left_w,
                area.y,
                fill,
                &"─".repeat(fill as usize),
                edge,
            );
        }
        put_text(buf, area.right() - 1, area.y, 1, "╮", edge);
        return (None, None);
    }

    put_text(buf, area.x, area.y, 1, "╭", edge);
    put_text(buf, area.right() - 1, area.y, 1, "╮", edge);
    let inner_w = usize::from(area.width - 2);
    if inner_w < 5 {
        return (None, None);
    }
    let right_cap = inner_w.saturating_sub(4);
    let right = truncate_end(&header.right, right_cap);
    let right_w = display_width(&right);
    let left_budget = inner_w.saturating_sub(right_w + 5);
    let left = truncate_end(&header.left, left_budget);
    let left_w = display_width(&left);

    put_text(buf, area.x + 1, area.y, 1, " ", edge);
    if left_w > 0 {
        put_text(
            buf,
            area.x + 2,
            area.y,
            left_w as u16,
            &left,
            edge.add_modifier(Modifier::BOLD),
        );
    }
    let after_left = area.x + 2 + left_w as u16;
    let right_x = area.right() - 2 - right_w as u16;
    if right_x > after_left + 1 {
        put_text(buf, after_left, area.y, 1, " ", edge);
        let dashes = (right_x - 1).saturating_sub(after_left + 1);
        if dashes > 0 {
            put_text(
                buf,
                after_left + 1,
                area.y,
                dashes,
                &"─".repeat(dashes as usize),
                edge,
            );
        }
        put_text(buf, right_x - 1, area.y, 1, " ", edge);
    }
    put_text(
        buf,
        right_x,
        area.y,
        right_w as u16,
        &right,
        Style::default().fg(palette.subtext0),
    );
    put_text(buf, area.right() - 2, area.y, 1, " ", edge);

    let mut prev_hit = None;
    let mut next_hit = None;
    if let Some(nav) = header.nav_token.as_deref()
        && let Some(pos) = right.find(nav)
    {
        let prefix_w = display_width(&right[..pos]) as u16;
        let nav_w = display_width(nav) as u16;
        if nav_w >= 3 {
            let px = right_x + prefix_w;
            let nx = px + nav_w - 1;
            prev_hit = Some(Rect::new(px, area.y, 1, 1));
            next_hit = Some(Rect::new(nx, area.y, 1, 1));
        }
    }
    (prev_hit, next_hit)
}

/// Draw Row 2 context (`from → to · runner · model · ■/□ state` and `task · file`).
pub(super) fn draw_context_row(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    header: &HeaderLayout,
    palette: &Palette,
) {
    if area.width <= 4 {
        return;
    }
    let avail = usize::from(area.width - 4);
    let dim = Style::default().fg(palette.subtext0);

    let right_full_w = display_width(&header.row2_right);
    let right_budget = if right_full_w == 0 {
        0
    } else if header.row2_left.is_empty() && header.row2_state.is_none() {
        avail
    } else {
        right_full_w.min(avail / 2)
    };
    let right = truncate_end(&header.row2_right, right_budget);
    let right_w = display_width(&right);
    if right_w > 0 {
        let rx = area.right() - 2 - right_w as u16;
        put_text(buf, rx, y, right_w as u16, &right, dim);
    }

    let left_budget = if right_w == 0 {
        avail
    } else {
        avail.saturating_sub(right_w + 2)
    };
    let mut cx = area.x + 2;
    let mut rem = left_budget;
    if !header.row2_left.is_empty() && rem > 0 {
        let txt = truncate_end(&header.row2_left, rem);
        let tw = display_width(&txt);
        put_text(buf, cx, y, tw as u16, &txt, dim);
        cx += tw as u16;
        rem = rem.saturating_sub(tw);
    }
    if let Some((glyph, st_style, word)) = header.row2_state_style() {
        let sep = if header.row2_left.is_empty() {
            ""
        } else {
            " · "
        };
        let sep_w = display_width(sep);
        let gw = display_width(glyph);
        if rem > sep_w + gw {
            if sep_w > 0 {
                put_text(buf, cx, y, sep_w as u16, sep, dim);
                cx += sep_w as u16;
                rem -= sep_w;
            }
            put_text(buf, cx, y, gw as u16, glyph, st_style);
            cx += gw as u16;
            rem -= gw;
            if rem > 1 {
                let tail = truncate_end(&format!(" {word}"), rem);
                let tw = display_width(&tail);
                put_text(buf, cx, y, tw as u16, &tail, dim);
            }
        }
    }
}

pub(super) fn draw_divider(buf: &mut Buffer, area: Rect, y: u16, edge: Style, palette: &Palette) {
    put_text(buf, area.x, y, 1, "├", edge);
    if area.width > 2 {
        put_text(
            buf,
            area.x + 1,
            y,
            area.width - 2,
            &"─".repeat(usize::from(area.width - 2)),
            Style::default()
                .fg(palette.elevation().border_muted)
                .bg(crate::chrome::popup_frame::overlay_bg(palette)),
        );
    }
    if area.width >= 2 {
        put_text(buf, area.right() - 1, y, 1, "┤", edge);
    }
}

pub(super) fn draw_bottom_border(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    scroll_range: Option<(usize, usize, usize)>,
    edge: Style,
    palette: &Palette,
) {
    put_text(buf, area.x, y, 1, "╰", edge);
    if area.width < 2 {
        return;
    }
    let mut used = 1u16;
    if let Some((a, b, total)) = scroll_range
        && area.width > 6
    {
        let label = format!(" lines {a}–{b} of {total} ");
        let max_lbl = usize::from(area.width - 3);
        let fitted = truncate_end(&label, max_lbl);
        let lbl_w = display_width(&fitted) as u16;
        put_text(buf, area.x + 1, y, 1, "─", edge);
        put_text(
            buf,
            area.x + 2,
            y,
            lbl_w,
            &fitted,
            Style::default().fg(palette.subtext0),
        );
        used = 2 + lbl_w;
    }
    let fill = (area.width - 1).saturating_sub(used);
    if fill > 0 {
        put_text(
            buf,
            area.x + used,
            y,
            fill,
            &"─".repeat(fill as usize),
            edge,
        );
    }
    put_text(buf, area.right() - 1, y, 1, "╯", edge);
}

pub(super) fn put_styled_line(buf: &mut Buffer, x: u16, y: u16, max_w: u16, line: &Line<'_>) {
    let mut cx = x;
    let right = x.saturating_add(max_w);
    for span in &line.spans {
        if cx >= right {
            break;
        }
        let text = span.content.as_ref();
        let rem = right.saturating_sub(cx);
        let w = display_width(text).min(usize::from(rem)) as u16;
        if w > 0 {
            put_text(buf, cx, y, rem, text, span.style);
            cx = cx.saturating_add(w);
        }
    }
}

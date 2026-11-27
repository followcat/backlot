//! Reader frame chrome: Row 1 border (`left` + right `◀ i/N ▶ · <age>`),
//! Row 2 semantic context row, `surface1` dividers, `surface0` action
//! strip, bottom scroll range, and styled span emitter.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;

use super::reader_meta::HeaderLayout;
use crate::chrome::palette::Palette;
use crate::chrome::popup_frame::overlay_bg;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};

/// Reader horizontal divider variant across plain, `< 64`, and stepped frames.
#[derive(Clone, Copy)]
pub(super) enum DividerKind {
    Plain,
    SolidDouble,
    StepTop,
    StepBottom,
    InnerDouble,
}

pub(super) fn babylon_rail_style(palette: &Palette) -> Style {
    palette.elevation().gilt_style(overlay_bg(palette))
}

/// Replace `popup_frame`'s single-line side rails with `║` across interior rows
/// when Babylon studio chrome is active.
pub(super) fn draw_vertical_rails(buf: &mut Buffer, area: Rect, palette: &Palette, plain: bool) {
    if plain || area.width < 2 || area.height < 3 {
        return;
    }
    let rail = babylon_rail_style(palette);
    for y in (area.y + 1)..(area.bottom() - 1) {
        put_text(buf, area.x, y, 1, "║", rail);
        put_text(buf, area.right() - 1, y, 1, "║", rail);
    }
}

struct TopBorderCaps {
    left: &'static str,
    right: &'static str,
    empty_right: &'static str,
    fill: &'static str,
    rail: Style,
    title: Style,
    right_style: Style,
}

fn top_border_caps(edge: Style, palette: &Palette, plain: bool, stepped: bool) -> TopBorderCaps {
    if plain {
        return TopBorderCaps {
            left: "╭ ",
            right: " ╮",
            empty_right: "╮",
            fill: "─",
            rail: edge,
            title: edge.add_modifier(Modifier::BOLD),
            right_style: Style::default().fg(palette.subtext0),
        };
    }
    let bg = overlay_bg(palette);
    let (left, right, empty_right) = if stepped {
        ("╔══╤═ ", " ═╤══╗", "═╤══╗")
    } else {
        ("╔═ ", " ═╗", "═╗")
    };
    TopBorderCaps {
        left,
        right,
        empty_right,
        fill: "═",
        rail: babylon_rail_style(palette),
        title: Style::default()
            .fg(palette.accent)
            .bg(bg)
            .add_modifier(Modifier::BOLD),
        right_style: Style::default().fg(palette.subtext0).bg(bg),
    }
}

fn put_repeat(buf: &mut Buffer, x: u16, y: u16, width: u16, ch: &str, style: Style) {
    if width > 0 {
        put_text(buf, x, y, width, &ch.repeat(usize::from(width)), style);
    }
}

/// Draw the top border with left title/kind badge and optional right
/// nav controls (`◀ i/N ▶ · <age>`), returning `(prev_hit, next_hit)`.
pub(super) fn draw_top_border(
    buf: &mut Buffer,
    area: Rect,
    header: &HeaderLayout,
    edge: Style,
    palette: &Palette,
    plain: bool,
    has_row2: bool,
) -> (Option<Rect>, Option<Rect>) {
    if area.width < 2 {
        return (None, None);
    }
    let stepped = !plain && area.width >= 64 && has_row2;
    let caps = top_border_caps(edge, palette, plain, stepped);
    let cap_l = display_width(caps.left) as u16;

    if header.right.is_empty() {
        let cap_r = display_width(caps.empty_right) as u16;
        let max_w = usize::from(area.width.saturating_sub(cap_l + cap_r + 1));
        let chosen = pick_title(header, max_w, plain);
        let tw = display_width(&chosen) as u16;
        put_text(
            buf,
            area.x,
            area.y,
            cap_l.min(area.width - 1),
            caps.left,
            caps.rail,
        );
        let (tx, right_x) = (area.x + cap_l, area.right().saturating_sub(cap_r));
        if tw > 0 && tx < right_x {
            put_text(buf, tx, area.y, tw.min(right_x - tx), &chosen, caps.title);
        }
        if tx + tw < right_x {
            put_text(buf, tx + tw, area.y, 1, " ", caps.rail);
            let fill = right_x.saturating_sub(tx + tw + 1);
            put_repeat(buf, tx + tw + 1, area.y, fill, caps.fill, caps.rail);
        }
        put_text(buf, right_x, area.y, cap_r, caps.empty_right, caps.rail);
        return (None, None);
    }

    let cap_r = display_width(caps.right) as u16;
    if area.width <= cap_l + cap_r + 4 {
        let (l, r) = if plain {
            ("╭", "╮")
        } else {
            ("╔", "╗")
        };
        put_text(buf, area.x, area.y, 1, l, caps.rail);
        put_repeat(
            buf,
            area.x + 1,
            area.y,
            area.width - 2,
            caps.fill,
            caps.rail,
        );
        put_text(buf, area.right() - 1, area.y, 1, r, caps.rail);
        return (None, None);
    }

    put_text(buf, area.x, area.y, cap_l, caps.left, caps.rail);
    put_text(
        buf,
        area.right() - cap_r,
        area.y,
        cap_r,
        caps.right,
        caps.rail,
    );

    let mid_w = usize::from(area.width - cap_l - cap_r);
    let right = truncate_end(&header.right, mid_w.saturating_sub(2));
    let right_w = display_width(&right);
    let left = pick_title(header, mid_w.saturating_sub(right_w + 3), plain);
    let left_w = display_width(&left);

    let left_x = area.x + cap_l;
    if left_w > 0 {
        put_text(buf, left_x, area.y, left_w as u16, &left, caps.title);
    }
    let after_left = left_x + left_w as u16;
    let right_x = area.right() - cap_r - right_w as u16;
    if right_x > after_left + 1 {
        put_text(buf, after_left, area.y, 1, " ", caps.rail);
        let dashes = (right_x - 1).saturating_sub(after_left + 1);
        put_repeat(buf, after_left + 1, area.y, dashes, caps.fill, caps.rail);
        put_text(buf, right_x - 1, area.y, 1, " ", caps.rail);
    }
    put_text(
        buf,
        right_x,
        area.y,
        right_w as u16,
        &right,
        caps.right_style,
    );
    nav_hit_rects(area.y, right_x, &right, header.nav_token.as_deref())
}

fn pick_title(header: &HeaderLayout, left_budget: usize, plain: bool) -> String {
    if !plain
        && let Some(wide) = header.left_wide.as_deref()
        && display_width(wide) <= left_budget
    {
        return wide.to_string();
    }
    truncate_end(&header.left, left_budget)
}

fn nav_hit_rects(
    y: u16,
    right_x: u16,
    right: &str,
    nav_token: Option<&str>,
) -> (Option<Rect>, Option<Rect>) {
    if let Some(nav) = nav_token
        && let Some(pos) = right.find(nav)
    {
        let prefix_w = display_width(&right[..pos]) as u16;
        let nav_w = display_width(nav) as u16;
        if nav_w >= 3 {
            let px = right_x + prefix_w;
            let nx = px + nav_w - 1;
            return (Some(Rect::new(px, y, 1, 1)), Some(Rect::new(nx, y, 1, 1)));
        }
    }
    (None, None)
}

/// Draw Row 2 context (`from → to · runner · model · ■/□ state` and `task · file`).
pub(super) fn draw_context_row(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    header: &HeaderLayout,
    palette: &Palette,
    plain: bool,
) {
    let stepped = !plain && area.width >= 64;
    let min_w = if stepped { 11 } else { 5 };
    if area.width < min_w {
        return;
    }
    if stepped {
        let rail = babylon_rail_style(palette);
        put_text(buf, area.x, y, 4, "║  │", rail);
        put_text(buf, area.right() - 4, y, 4, "│  ║", rail);
    }
    let inset: u16 = if stepped { 5 } else { 2 };
    let avail = usize::from(area.width.saturating_sub(2 * inset));
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
        let rx = area.right() - inset - right_w as u16;
        put_text(buf, rx, y, right_w as u16, &right, dim);
    }

    let left_budget = if right_w == 0 {
        avail
    } else {
        avail.saturating_sub(right_w + 2)
    };
    let mut cx = area.x + inset;
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

pub(super) fn draw_frame_divider(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    edge: Style,
    palette: &Palette,
    plain: bool,
    top: bool,
) {
    let kind = if plain {
        DividerKind::Plain
    } else if area.width < 64 {
        DividerKind::SolidDouble
    } else if top {
        DividerKind::StepTop
    } else {
        DividerKind::StepBottom
    };
    draw_divider(buf, area, y, edge, palette, kind);
}

pub(super) fn draw_divider(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    edge: Style,
    palette: &Palette,
    kind: DividerKind,
) {
    let bg = overlay_bg(palette);
    let gilt = babylon_rail_style(palette);
    let muted = Style::default().fg(palette.elevation().border_muted).bg(bg);
    let (left_cap, right_cap, cap_style, mid_style) = match kind {
        DividerKind::Plain => ("├", "┤", edge, muted),
        DividerKind::SolidDouble => ("╟", "╢", gilt, gilt),
        DividerKind::StepTop => ("╟──┴", "┴──╢", gilt, gilt),
        DividerKind::StepBottom => ("╟──┬", "┬──╢", gilt, gilt),
        DividerKind::InnerDouble => ("╟", "╢", gilt, muted),
    };
    let cw = display_width(left_cap) as u16;
    if area.width < cw * 2 {
        return;
    }
    put_text(buf, area.x, y, cw, left_cap, cap_style);
    put_repeat(buf, area.x + cw, y, area.width - cw * 2, "─", mid_style);
    put_text(buf, area.right() - cw, y, cw, right_cap, cap_style);
}

pub(super) fn draw_bottom_border(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    scroll_range: Option<(usize, usize, usize)>,
    edge: Style,
    palette: &Palette,
    plain: bool,
) {
    if area.width < 2 {
        if area.width == 1 {
            put_text(buf, area.x, y, 1, if plain { "╰" } else { "╚" }, edge);
        }
        return;
    }
    let stepped = !plain && area.width >= 64;
    let rail = if plain {
        edge
    } else {
        babylon_rail_style(palette)
    };
    let lbl_style = Style::default().fg(palette.subtext0);
    let lbl_style = if plain {
        lbl_style
    } else {
        lbl_style.bg(overlay_bg(palette))
    };
    let (left_empty, left_lbl, right_cap, fill_ch) = if plain {
        ("╰", "╰─", "╯", "─")
    } else if stepped {
        ("╚══╧", "╚══╧═", "╧══╝", "═")
    } else {
        ("╚", "╚═", "╝", "═")
    };
    let cap_r = display_width(right_cap) as u16;
    let lbl_left_w = display_width(left_lbl) as u16;

    let used = if let Some((a, b, total)) = scroll_range
        && area.width > lbl_left_w + cap_r + 2
    {
        put_text(buf, area.x, y, lbl_left_w, left_lbl, rail);
        let label = format!(" lines {a}–{b} of {total} ");
        let fitted = truncate_end(&label, usize::from(area.width - lbl_left_w - cap_r));
        let lbl_w = display_width(&fitted) as u16;
        put_text(buf, area.x + lbl_left_w, y, lbl_w, &fitted, lbl_style);
        lbl_left_w + lbl_w
    } else {
        let w = display_width(left_empty) as u16;
        put_text(buf, area.x, y, w, left_empty, rail);
        w
    };

    let fill = (area.width - cap_r).saturating_sub(used);
    put_repeat(buf, area.x + used, y, fill, fill_ch, rail);
    put_text(buf, area.right() - cap_r, y, cap_r, right_cap, rail);
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

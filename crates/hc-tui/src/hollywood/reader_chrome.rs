//! Reader chrome coordinator and semantic context row (`Row 2` + lower
//! action/compose strips + styled line emitter). Border rails live in
//! `reader_frame.rs`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;

use super::reader_chips::draw_action_strip;
use super::reader_compose::{ReaderComposeOpts, draw_compose_band, draw_composing_hints};
use super::reader_frame::{
    babylon_rail_style, draw_bottom_border, draw_frame_divider, draw_top_border,
    draw_vertical_rails,
};
use super::reader_meta::HeaderLayout;
use super::view::{CardView, ViewRender};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};

/// Frame rendering inputs shared across the top and bottom Reader chrome passes.
pub(super) struct ChromePass<'a> {
    pub view: &'a CardView,
    pub header: &'a HeaderLayout,
    pub edge: Style,
    pub palette: &'a Palette,
    pub plain: bool,
    pub show_row2: bool,
}

/// Bottom strip & inline compose geometry passed to [`draw_lower_chrome`].
pub(super) struct LowerChrome<'a> {
    pub div_y: u16,
    pub bottom: u16,
    pub has_compose: bool,
    pub nav_total: usize,
    pub pending: Option<&'a str>,
    pub compose: Option<ReaderComposeOpts<'a>>,
}

/// Draw vertical rails, top border (and Row 2 context + top divider), or the
/// collapsed bottom edge when `area.height < 4`.
pub(super) fn draw_top_chrome(
    buf: &mut Buffer,
    area: Rect,
    bottom: u16,
    pass: &ChromePass<'_>,
) -> (Option<Rect>, Option<Rect>) {
    draw_vertical_rails(buf, area, pass.palette, pass.plain);
    let hits = draw_top_border(
        buf,
        area,
        pass.header,
        pass.edge,
        pass.palette,
        pass.plain,
        pass.show_row2,
    );
    if area.height < 4 {
        draw_bottom_border(buf, area, bottom, None, pass.edge, pass.palette, pass.plain);
    } else if pass.show_row2 {
        draw_context_row(buf, area, area.y + 1, pass.header, pass.palette, pass.plain);
        draw_frame_divider(
            buf,
            area,
            area.y + 2,
            pass.edge,
            pass.palette,
            pass.plain,
            true,
        );
    }
    hits
}

/// Draw the bottom divider, action chips or composing hints, and optional inline
/// compose band, returning the exclusive bottom `y` of the body viewport.
pub(super) fn draw_lower_chrome(
    buf: &mut Buffer,
    area: Rect,
    pass: &ChromePass<'_>,
    lower: LowerChrome<'_>,
    hits: &mut ViewRender,
) -> u16 {
    let action_y = lower.bottom - 1;
    draw_frame_divider(
        buf,
        area,
        lower.div_y,
        pass.edge,
        pass.palette,
        pass.plain,
        false,
    );
    if pass.view.tell.is_composing() {
        draw_composing_hints(
            buf,
            area,
            action_y,
            lower.compose.is_none_or(|c| c.studio),
            pass.palette,
            pass.plain,
        );
    } else {
        draw_action_strip(
            buf,
            Rect::new(area.x, action_y, area.width, 1),
            pass.view,
            lower.nav_total,
            lower.pending,
            (pass.palette, pass.plain),
            &mut hits.chips,
        );
    }

    if lower.has_compose && lower.div_y >= area.y + 3 {
        let compose_div_y = lower.div_y - 2;
        let (crect, cursor) = draw_compose_band(
            buf,
            area,
            compose_div_y,
            &pass.view.tell,
            lower.compose,
            (pass.edge, pass.palette, pass.plain),
        );
        hits.compose_rect = crect;
        hits.cursor = cursor;
        compose_div_y
    } else {
        lower.div_y
    }
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

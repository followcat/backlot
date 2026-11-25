//! The Reader card (`spec.md` §1-2): adaptive content-sized card with
//! metadata header, width-accurate CJK/Markdown/log wrapping, scrollbar,
//! and action strip.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::reader_chrome::{
    draw_action_strip, draw_bottom_border, draw_context_row, draw_divider, draw_top_border,
    put_styled_line,
};
use super::reader_meta::{HeaderLayout, ReaderMeta};
use super::reader_wrap::{body_mode_for_title, natural_body_width, render_body_lines};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::scroll::{ScrollMetrics, list_scroll_metrics, render_list_scrollbar};
use crate::chrome::text::display_width;

/// The open view: `card` is `Some(id)` when showing an actionable
/// card (`a`/`x` apply/cancel it) and `None` for read-only views
/// (`hollywood show`, `report show`, `records`).
#[derive(Debug, Default)]
pub(crate) struct CardView {
    pub card: Option<String>,
    pub title: String,
    pub lines: Vec<String>,
    pub scroll: usize,
    pub meta: ReaderMeta,
}

/// What the mouse needs: the box's rect, the scrollable body, the
/// scrollbar track when the text overflows, and header nav hit rects.
#[derive(Debug, Default)]
pub(crate) struct ViewRender {
    pub area: Rect,
    pub body: Rect,
    pub scrollbar: Rect,
    pub prev_hit: Option<Rect>,
    pub next_hit: Option<Rect>,
    pub metrics: Option<ScrollMetrics>,
}

/// Draw the adaptive content-sized Reader centered inside `surface` using wall-clock time.
pub(crate) fn render_view(
    buf: &mut Buffer,
    surface: Rect,
    view: &CardView,
    pending: Option<&str>,
    palette: &Palette,
) -> ViewRender {
    render_view_at(
        buf,
        surface,
        view,
        pending,
        palette,
        crate::update_state::now_ms(),
    )
}

/// Draw the adaptive content-sized Reader centered inside `surface` relative to `now_ms`.
pub(crate) fn render_view_at(
    buf: &mut Buffer,
    surface: Rect,
    view: &CardView,
    pending: Option<&str>,
    palette: &Palette,
    now_ms: u64,
) -> ViewRender {
    let mut hits = ViewRender::default();
    if surface.is_empty() {
        return hits;
    }

    let header = HeaderLayout::build(view, now_ms, palette);
    let path_hint = view.meta.file.as_deref().unwrap_or(&view.title);
    let mode = body_mode_for_title(path_hint);
    let min_w = 52u16.min(surface.width);
    let max_w = surface.width.saturating_sub(4).min(92).max(min_w);

    let row1_w = display_width(&header.left)
        + 6
        + if header.right.is_empty() {
            0
        } else {
            display_width(&header.right) + 3
        };
    let row2_state_w = header
        .row2_state
        .as_ref()
        .map_or(0, |(w, g, _)| display_width(g) + 1 + display_width(w) + 3);
    let row2_w = display_width(&header.row2_left)
        + row2_state_w
        + if header.row2_right.is_empty() {
            0
        } else {
            display_width(&header.row2_right) + 2
        }
        + 4;
    let header_w = row1_w.max(row2_w).min(max_w as usize);

    let body_nat_w = natural_body_width(&view.lines, mode);
    let base_w = body_nat_w
        .max(header_w)
        .saturating_add(4)
        .clamp(min_w as usize, max_w as usize) as u16;

    // Chrome rows: 4 without secondary context row, 6 with Row 2 + divider.
    let show_row2 = header.has_row2 && surface.height >= 8;
    let chrome_h: u16 = if show_row2 { 6 } else { 4 };
    let min_h = 8u16.min(surface.height);
    let max_h = surface.height.saturating_sub(2).min(32).max(min_h);
    let body_cap = max_h.saturating_sub(chrome_h).max(1);

    let initial_inner_w = base_w.saturating_sub(4).max(1) as usize;
    let mut wrapped = render_body_lines(&view.lines, mode, initial_inner_w, palette);
    let needs_bar = wrapped.len() > body_cap as usize;
    let width = if needs_bar {
        body_nat_w
            .max(header_w)
            .saturating_add(5)
            .clamp(min_w as usize, max_w as usize) as u16
    } else {
        base_w
    };
    if needs_bar {
        let bar_inner_w = width.saturating_sub(5).max(1) as usize;
        wrapped = render_body_lines(&view.lines, mode, bar_inner_w, palette);
    }

    let total_lines = wrapped.len();
    let height =
        (chrome_h as usize + total_lines.max(1)).clamp(min_h as usize, max_h as usize) as u16;
    let area = Rect::new(
        surface.x + (surface.width.saturating_sub(width)) / 2,
        surface.y + (surface.height.saturating_sub(height)) / 2,
        width,
        height,
    );
    hits.area = area;
    if area.is_empty() {
        return hits;
    }

    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            buf[(x, y)].reset();
        }
    }
    buf.set_style(area, Style::default().bg(palette.panel_bg));

    let edge = Style::default().fg(palette.accent);
    let (prev_hit, next_hit) = draw_top_border(buf, area, &header, edge, palette);
    hits.prev_hit = prev_hit;
    hits.next_hit = next_hit;

    let bottom = area.bottom() - 1;
    for y in area.y + 1..bottom {
        put_text(buf, area.x, y, 1, "│", edge);
        if area.width >= 2 {
            put_text(buf, area.right() - 1, y, 1, "│", edge);
        }
    }

    if area.height < 4 {
        draw_bottom_border(buf, area, bottom, None, edge, palette);
        return hits;
    }

    let show_row2 = show_row2 && area.height >= 6;
    if show_row2 {
        draw_context_row(buf, area, area.y + 1, &header, palette);
        draw_divider(buf, area, area.y + 2, edge, palette);
    }

    let action_y = bottom - 1;
    let div_y = bottom - 2;
    draw_divider(buf, area, div_y, edge, palette);
    draw_action_strip(buf, area, action_y, view.card.is_some(), pending, palette);

    let body_y = area.y + if show_row2 { 3 } else { 1 };
    let body_h = div_y.saturating_sub(body_y);
    if body_h == 0 || area.width <= 4 {
        draw_bottom_border(buf, area, bottom, None, edge, palette);
        return hits;
    }

    let body = Rect::new(area.x + 1, body_y, area.width - 2, body_h);
    hits.body = body;
    let heights: Vec<u16> = wrapped.iter().map(|_| 1).collect();
    let metrics = list_scroll_metrics(&heights, &[], body.height, view.scroll);
    hits.metrics = Some(metrics);
    let start = metrics.max_offset_from_bottom - metrics.offset_from_bottom;
    let show_bar = metrics.max_offset_from_bottom > 0;
    let content_width = area
        .width
        .saturating_sub(4 + u16::from(show_bar && body.width > 1));

    for (i, line) in wrapped.iter().enumerate().skip(start) {
        let y = body.y + (i - start) as u16;
        if y >= body.bottom() {
            break;
        }
        put_styled_line(buf, area.x + 2, y, content_width, line);
    }
    if show_bar {
        hits.scrollbar = Rect::new(body.right() - 1, body.y, 1, body.height);
        render_list_scrollbar(buf, hits.scrollbar, metrics, palette);
    }

    let scroll_range = show_bar.then(|| {
        let a = start + 1;
        let b = (start + body.height as usize).min(total_lines);
        (a, b, total_lines)
    });
    draw_bottom_border(buf, area, bottom, scroll_range, edge, palette);
    hits
}

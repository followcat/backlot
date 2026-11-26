//! The Reader card (`spec.md` §1-2): adaptive content-sized card with
//! metadata header, width-accurate CJK/Markdown/log wrapping, scrollbar,
//! and action strip.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::reader_chips::draw_action_strip;
use super::reader_chrome::{
    draw_bottom_border, draw_context_row, draw_divider, draw_top_border, put_styled_line,
};
use super::reader_compose::{
    InlineTellState, ReaderComposeOpts, draw_compose_band, draw_composing_hints,
};
use super::reader_meta::{HeaderLayout, ReaderMeta};
use super::reader_wrap::{body_mode_for_title, natural_body_width, render_body_lines};
use crate::chrome::help::CursorState;
use crate::chrome::palette::Palette;
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
    pub raw_body: String,
    pub scroll: usize,
    pub meta: ReaderMeta,
    pub tell: InlineTellState,
}

/// What the mouse needs: the box's rect, the scrollable body, the
/// scrollbar track when the text overflows, and header nav / action chip / compose rects.
#[derive(Debug, Default)]
pub(crate) struct ViewRender {
    pub area: Rect,
    pub body: Rect,
    pub scrollbar: Rect,
    pub prev_hit: Option<Rect>,
    pub next_hit: Option<Rect>,
    pub chips: Vec<(Rect, char)>,
    pub compose_rect: Option<Rect>,
    pub cursor: Option<CursorState>,
    pub metrics: Option<ScrollMetrics>,
}

/// Optional compose state, timestamp, and shadow toggle for `render_view_with_opts`.
pub(crate) struct ReaderRenderOpts<'a> {
    pub compose: Option<ReaderComposeOpts<'a>>,
    pub now_ms: u64,
    pub shadow: bool,
}

/// Draw the adaptive content-sized Reader centered inside `surface` using wall-clock time.
#[cfg(test)]
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
#[cfg(test)]
pub(crate) fn render_view_at(
    buf: &mut Buffer,
    surface: Rect,
    view: &CardView,
    pending: Option<&str>,
    palette: &Palette,
    now_ms: u64,
) -> ViewRender {
    render_view_with_opts(
        buf,
        surface,
        view,
        pending,
        palette,
        ReaderRenderOpts {
            compose: None,
            now_ms,
            shadow: true,
        },
    )
}

/// Draw the Reader with optional live inline compose editor context and cursor output.
#[cfg(test)]
pub(crate) fn render_view_with_compose(
    buf: &mut Buffer,
    surface: Rect,
    view: &CardView,
    pending: Option<&str>,
    compose: Option<ReaderComposeOpts<'_>>,
    palette: &Palette,
    now_ms: u64,
) -> ViewRender {
    render_view_with_opts(
        buf,
        surface,
        view,
        pending,
        palette,
        ReaderRenderOpts {
            compose,
            now_ms,
            shadow: true,
        },
    )
}

/// Draw the Reader with configurable compose, timestamp, and drop shadow.
pub(crate) fn render_view_with_opts(
    buf: &mut Buffer,
    surface: Rect,
    view: &CardView,
    pending: Option<&str>,
    palette: &Palette,
    opts: ReaderRenderOpts<'_>,
) -> ViewRender {
    let ReaderRenderOpts {
        compose,
        now_ms,
        shadow,
    } = opts;
    let mut hits = ViewRender::default();
    if surface.is_empty() {
        return hits;
    }

    let header = HeaderLayout::build(view, now_ms, palette);
    let path_hint = view.meta.file.as_deref().unwrap_or(&view.title);
    let mode = body_mode_for_title(path_hint);
    let min_w = 52u16.min(surface.width);
    let body_px: u16 = if surface.width >= 56 { 2 } else { 1 };
    let h_chrome = 2 + 2 * body_px;
    let max_w = surface.width.saturating_sub(2).min(94).max(min_w);

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
        .saturating_add(usize::from(h_chrome))
        .clamp(min_w as usize, max_w as usize) as u16;

    // Chrome rows: 4 without secondary context row, 6 with Row 2 + divider, +2 for compose band.
    let show_row2 = header.has_row2 && surface.height >= 8;
    let has_compose = view.tell.has_band() && surface.height >= 8;
    let base_chrome_h: u16 = if show_row2 { 6 } else { 4 };
    let compose_h: u16 = if has_compose { 2 } else { 0 };
    let chrome_h: u16 = base_chrome_h + compose_h;
    let min_h = (chrome_h + 1).min(surface.height);
    let max_h = surface.height.saturating_sub(2).min(34).max(min_h);
    let body_py: u16 = if max_h >= chrome_h + 6 { 1 } else { 0 };
    let v_pad = 2 * body_py;
    let body_cap = max_h.saturating_sub(chrome_h + v_pad).max(1);

    let initial_inner_w = base_w.saturating_sub(h_chrome).max(1) as usize;
    let mut wrapped = render_body_lines(&view.lines, mode, initial_inner_w, palette);
    let needs_bar = wrapped.len() > body_cap as usize;
    let width = if needs_bar {
        body_nat_w
            .max(header_w)
            .saturating_add(usize::from(h_chrome) + 1)
            .clamp(min_w as usize, max_w as usize) as u16
    } else {
        base_w
    };
    if needs_bar {
        let bar_inner_w = width.saturating_sub(h_chrome + 1).max(1) as usize;
        wrapped = render_body_lines(&view.lines, mode, bar_inner_w, palette);
    }

    let total_lines = wrapped.len();
    let desired_body_h = total_lines.max(1) + usize::from(v_pad);
    let height = (chrome_h as usize + desired_body_h).clamp(min_h as usize, max_h as usize) as u16;
    let Some(geom) = crate::chrome::popup_frame::open_from_border_size(
        buf,
        surface,
        width,
        height,
        4.min(surface.width),
        3.min(surface.height),
        palette.accent,
        crate::chrome::popup_frame::PopupChrome::BorderJoined,
        palette,
        shadow,
    ) else {
        return hits;
    };
    let area = geom.border;
    hits.area = geom.outer;
    if area.is_empty() {
        return hits;
    }

    let bg = crate::chrome::popup_frame::overlay_bg(palette);
    let edge = Style::default().fg(palette.accent).bg(bg);
    let (prev_hit, next_hit) = draw_top_border(buf, area, &header, edge, palette);
    hits.prev_hit = prev_hit;
    hits.next_hit = next_hit;

    let bottom = area.bottom() - 1;
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
    if view.tell.is_composing() {
        draw_composing_hints(
            buf,
            area,
            action_y,
            compose.is_none_or(|c| c.studio),
            palette,
        );
    } else {
        draw_action_strip(buf, area, action_y, view, pending, palette, &mut hits.chips);
    }

    let body_end = if has_compose && div_y >= area.y + 3 {
        let compose_div_y = div_y - 2;
        let (crect, cursor) =
            draw_compose_band(buf, area, compose_div_y, &view.tell, compose, edge, palette);
        hits.compose_rect = crect;
        hits.cursor = cursor;
        compose_div_y
    } else {
        div_y
    };

    let raw_body_y = area.y + if show_row2 { 3 } else { 1 };
    let raw_body_h = body_end.saturating_sub(raw_body_y);
    if raw_body_h == 0 || area.width <= 4 {
        draw_bottom_border(buf, area, bottom, None, edge, palette);
        return hits;
    }

    let actual_px: u16 = if area.width >= 8 + body_px * 2 {
        body_px
    } else {
        1
    };
    let actual_py: u16 =
        if raw_body_h >= 4 && (usize::from(raw_body_h) >= total_lines + 2 || raw_body_h >= 6) {
            1
        } else {
            0
        };
    let raw_body = Rect::new(area.x + 1, raw_body_y, area.width - 2, raw_body_h);
    let body = Rect::new(
        raw_body.x,
        raw_body_y + actual_py,
        raw_body.width,
        raw_body_h.saturating_sub(2 * actual_py),
    );
    hits.body = raw_body;
    let heights: Vec<u16> = wrapped.iter().map(|_| 1).collect();
    let metrics = list_scroll_metrics(&heights, &[], body.height, view.scroll);
    hits.metrics = Some(metrics);
    let start = metrics.max_offset_from_bottom - metrics.offset_from_bottom;
    let show_bar = metrics.max_offset_from_bottom > 0;
    let content_width = area
        .width
        .saturating_sub(2 + 2 * actual_px + u16::from(show_bar && raw_body.width > 1));

    for (i, line) in wrapped.iter().enumerate().skip(start) {
        let y = body.y + (i - start) as u16;
        if y >= body.bottom() {
            break;
        }
        put_styled_line(buf, area.x + 1 + actual_px, y, content_width, line);
    }
    if show_bar {
        hits.scrollbar = Rect::new(raw_body.right() - 1, body.y, 1, body.height);
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

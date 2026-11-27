//! The Reader card (`spec.md` §1-2): adaptive content-sized card with
//! metadata header, width-accurate CJK/Markdown/log wrapping, scrollbar,
//! and action strip.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::reader_chrome::{
    ChromePass, LowerChrome, draw_lower_chrome, draw_top_chrome, put_styled_line,
};
use super::reader_compose::{InlineTellState, ReaderComposeOpts};
use super::reader_frame::draw_bottom_border;
use super::reader_md::parse_design_sections;
use super::reader_meta::{HeaderLayout, ReaderKind, ReaderMeta};
use super::reader_wrap::{body_mode_for_title, natural_body_width, render_body_with_sections};
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
    pub sections: Vec<(String, usize)>,
    pub active_section: usize,
}

/// Optional compose state, timestamp, shadow, and plain-mode toggle for `render_view_with_opts`.
#[derive(Default)]
pub(crate) struct ReaderRenderOpts<'a> {
    pub compose: Option<ReaderComposeOpts<'a>>,
    pub now_ms: u64,
    pub shadow: bool,
    pub plain: bool,
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
            plain: false,
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
            plain: false,
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
        plain,
    } = opts;
    let mut hits = ViewRender::default();
    if surface.is_empty() {
        return hits;
    }

    let mut meta_for_header = view.meta.clone();
    if view.meta.kind == ReaderKind::Design && meta_for_header.nav_total.is_none() {
        let pre_sections = parse_design_sections(&view.lines);
        if let Some(widest) = pre_sections.iter().max_by_key(|s| display_width(&s.label)) {
            meta_for_header.nav_index = Some(pre_sections.len());
            meta_for_header.nav_total = Some(pre_sections.len());
            meta_for_header.design_section = Some(widest.label.clone());
        }
    }
    let mut header = HeaderLayout::build(
        &view.title,
        view.card.as_deref(),
        &meta_for_header,
        now_ms,
        palette,
        plain,
    );
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
    let (mut wrapped, mut sections) =
        render_body_with_sections(&view.lines, mode, initial_inner_w, palette, plain);
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
        (wrapped, sections) =
            render_body_with_sections(&view.lines, mode, bar_inner_w, palette, plain);
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

    let bottom = area.bottom() - 1;
    let show_row2_actual = show_row2 && area.height >= 6;
    let div_y = bottom.saturating_sub(2);
    let compose_body_end = if has_compose && div_y >= area.y + 3 {
        div_y - 2
    } else {
        div_y
    };
    let raw_body_y_pre = area.y + if show_row2_actual { 3 } else { 1 };
    let raw_body_h_pre = compose_body_end.saturating_sub(raw_body_y_pre);
    let pre_py: u16 = body_py.min(raw_body_h_pre / 2);
    let body_h_pre = raw_body_h_pre.saturating_sub(2 * pre_py);
    let heights: Vec<u16> = wrapped.iter().map(|_| 1).collect();
    let metrics = list_scroll_metrics(&heights, &[], body_h_pre, view.scroll);
    let start = metrics.max_offset_from_bottom - metrics.offset_from_bottom;
    if view.meta.kind == ReaderKind::Design && !sections.is_empty() {
        let active = view
            .meta
            .design_section_idx
            .map(|i| i.min(sections.len() - 1))
            .unwrap_or_else(|| {
                sections
                    .iter()
                    .rposition(|(_, ln)| *ln <= start)
                    .unwrap_or(0)
            });
        meta_for_header.nav_index = Some(active + 1);
        meta_for_header.nav_total = Some(sections.len());
        meta_for_header.design_section = Some(sections[active].0.clone());
        header = HeaderLayout::build(
            &view.title,
            view.card.as_deref(),
            &meta_for_header,
            now_ms,
            palette,
            plain,
        );
        hits.active_section = active;
        hits.sections = sections;
    }

    let bg = crate::chrome::popup_frame::overlay_bg(palette);
    let edge = Style::default().fg(palette.accent).bg(bg);
    let pass = ChromePass {
        view,
        header: &header,
        edge,
        palette,
        plain,
        show_row2: show_row2_actual,
    };
    let (prev_hit, next_hit) = draw_top_chrome(buf, area, bottom, &pass);
    hits.prev_hit = prev_hit;
    hits.next_hit = next_hit;
    if area.height < 4 {
        return hits;
    }

    let show_row2 = show_row2_actual;
    let body_end = draw_lower_chrome(
        buf,
        area,
        &pass,
        LowerChrome {
            div_y,
            bottom,
            has_compose,
            nav_total: meta_for_header.nav_total.unwrap_or(0),
            pending,
            compose,
        },
        &mut hits,
    );

    let raw_body_y = area.y + if show_row2 { 3 } else { 1 };
    let raw_body_h = body_end.saturating_sub(raw_body_y);
    if raw_body_h == 0 || area.width <= 4 {
        draw_bottom_border(buf, area, bottom, None, edge, palette, plain);
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
    hits.metrics = Some(metrics);
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
    draw_bottom_border(buf, area, bottom, scroll_range, edge, palette, plain);
    hits
}

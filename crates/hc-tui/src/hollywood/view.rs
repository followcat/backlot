//! The Reader card (`spec.md` §1-2): adaptive content-sized card with
//! metadata header, width-accurate CJK/Markdown/log wrapping, scrollbar,
//! and action strip.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;

use super::reader_wrap::{body_mode_for_title, natural_body_width, render_body_lines};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::scroll::{ScrollMetrics, list_scroll_metrics, render_list_scrollbar};
use crate::chrome::text::{display_width, fit_hints, truncate_end};

/// The open view: `card` is `Some(id)` when showing an actionable
/// card (`a`/`x` apply/cancel it) and `None` for read-only views
/// (`hollywood show`, `report show`, `records`).
#[derive(Debug)]
pub(crate) struct CardView {
    pub card: Option<String>,
    pub title: String,
    pub lines: Vec<String>,
    pub scroll: usize,
}

/// What the mouse needs: the box's rect, the scrollable body and the
/// scrollbar track when the text overflows.
#[derive(Debug, Default)]
pub(crate) struct ViewRender {
    pub area: Rect,
    pub body: Rect,
    pub scrollbar: Rect,
    pub metrics: Option<ScrollMetrics>,
}

/// Draw the adaptive content-sized Reader centered inside `surface`.
pub(crate) fn render_view(
    buf: &mut Buffer,
    surface: Rect,
    view: &CardView,
    pending: Option<&str>,
    palette: &Palette,
) -> ViewRender {
    let mut hits = ViewRender::default();
    if surface.is_empty() {
        return hits;
    }

    let mode = body_mode_for_title(&view.title);
    let min_w = 52u16.min(surface.width);
    let max_w = surface.width.saturating_sub(4).min(92).max(min_w);
    let header_w = (display_width(&view.title) + 6).min(max_w as usize);
    let body_nat_w = natural_body_width(&view.lines, mode);
    let base_w = body_nat_w
        .max(header_w)
        .saturating_add(4)
        .clamp(min_w as usize, max_w as usize) as u16;

    // Chrome rows without secondary metadata: top border, divider above
    // action strip, action strip, bottom border (`chrome_h = 4`).
    let chrome_h = 4u16;
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
    draw_top_border(buf, area, &view.title, edge);

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

    let action_y = bottom - 1;
    let div_y = bottom - 2;
    draw_divider(buf, area, div_y, edge, palette);
    draw_action_strip(buf, area, action_y, view.card.is_some(), pending, palette);

    let body_y = area.y + 1;
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

fn draw_top_border(buf: &mut Buffer, area: Rect, title: &str, edge: Style) {
    if area.width < 2 {
        return;
    }
    let max_title_w = (area.width as usize).saturating_sub(4);
    let left = format!("╭ {} ", truncate_end(title, max_title_w));
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
}

fn draw_divider(buf: &mut Buffer, area: Rect, y: u16, edge: Style, palette: &Palette) {
    put_text(buf, area.x, y, 1, "├", edge);
    if area.width > 2 {
        put_text(
            buf,
            area.x + 1,
            y,
            area.width - 2,
            &"─".repeat(usize::from(area.width - 2)),
            Style::default().fg(palette.surface1),
        );
    }
    if area.width >= 2 {
        put_text(buf, area.right() - 1, y, 1, "┤", edge);
    }
}

fn draw_action_strip(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    is_card: bool,
    pending: Option<&str>,
    palette: &Palette,
) {
    if area.width <= 2 {
        return;
    }
    let strip = Rect::new(area.x + 1, y, area.width - 2, 1);
    let bg = Style::default().bg(palette.surface0);
    buf.set_style(strip, bg);
    let avail = usize::from(area.width.saturating_sub(4));
    let label = match (is_card, pending) {
        (true, Some(lbl)) => truncate_end(&format!("… {lbl}"), avail),
        (true, None) => fit_hints(&["a apply  x cancel  esc back", "a x  esc"], avail).to_string(),
        (false, _) => fit_hints(&["↑↓ scroll  esc back", "↑↓  esc"], avail).to_string(),
    };
    put_text(
        buf,
        area.x + 2,
        y,
        avail as u16,
        &label,
        bg.fg(palette.subtext0),
    );
}

fn draw_bottom_border(
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

fn put_styled_line(buf: &mut Buffer, x: u16, y: u16, max_w: u16, line: &Line<'_>) {
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

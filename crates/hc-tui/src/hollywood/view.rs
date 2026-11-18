//! The "Show me" read (D16): a `studio-new` card's `card show` output
//! — the draft's cast and brief — in a read-only bordered box over
//! the layer. `a` applies the card, `x` cancels it — the same
//! commands and pending mark as the row — `Esc`, `Enter` or a click
//! outside closes it; the wheel and arrows scroll it. Client-local
//! only: the text is whatever the `backlot` command printed.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::scroll::{ScrollMetrics, list_scroll_metrics, render_list_scrollbar};
use crate::chrome::text::truncate_end;

/// The open view: the card's id in the title, the command's stdout
/// as scrollable lines. `card` is the id `a`/`x` act on.
#[derive(Debug)]
pub(crate) struct CardView {
    pub card: String,
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

/// The view's key line — `a`/`x` answer the card, not close it.
const HINTS: &str = "a apply  x cancel  esc back";

/// Draw the view centered over the frame — at most the frame's
/// middle 7/8, at least a 20×6 box when the frame has room. `pending`
/// is the running card action's label: the footer trades the hints
/// for `…` until the result lands.
pub(crate) fn render_view(
    buf: &mut Buffer,
    frame: Rect,
    view: &CardView,
    pending: Option<&str>,
    palette: &Palette,
) -> ViewRender {
    let width = (frame.width * 7 / 8).clamp(20.min(frame.width), frame.width);
    let height = (frame.height * 7 / 8).clamp(6.min(frame.height), frame.height);
    let area = Rect::new(
        frame.x + (frame.width - width) / 2,
        frame.y + (frame.height - height) / 2,
        width,
        height,
    );
    let mut hits = ViewRender {
        area,
        ..ViewRender::default()
    };
    if area.is_empty() {
        return hits;
    }
    // Reset the box first so pane cells cannot bleed through, then
    // the panel background — the same treatment the layer gets.
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            buf[(x, y)].reset();
        }
    }
    buf.set_style(area, Style::default().bg(palette.panel_bg));

    let edge = Style::default().fg(palette.accent);
    // `╭ title ────╮` — the title in the top edge, `─` filling what
    // it leaves, `╮` closing it.
    let title = format!("╭ {} ", truncate_end(&view.title, area.width as usize - 4));
    let title_w = title.chars().count() as u16;
    put_text(
        buf,
        area.x,
        area.y,
        area.width,
        &title,
        edge.add_modifier(Modifier::BOLD),
    );
    if area.width > title_w + 1 {
        put_text(
            buf,
            area.x + title_w,
            area.y,
            area.width - title_w - 1,
            &"─".repeat(usize::from(area.width - title_w - 1)),
            edge,
        );
    }
    if area.width >= 2 {
        put_text(buf, area.x + area.width - 1, area.y, 1, "╮", edge);
    }
    // `│` on both sides, then `╰ hints ──╯` on the bottom — a
    // complete box over the band.
    let bottom = area.y + area.height - 1;
    for y in area.y + 1..bottom {
        put_text(buf, area.x, y, 1, "│", edge);
        if area.width >= 2 {
            put_text(buf, area.x + area.width - 1, y, 1, "│", edge);
        }
    }
    put_text(buf, area.x, bottom, 1, "╰", edge);
    let hints = match pending {
        Some(label) => format!("… {label}"),
        None => HINTS.to_string(),
    };
    if area.width > 2 {
        let foot = format!(" {} ", truncate_end(&hints, area.width as usize - 4));
        let foot_w = foot.chars().count() as u16;
        put_text(
            buf,
            area.x + 1,
            bottom,
            foot_w.min(area.width - 2),
            &foot,
            edge.fg(palette.subtext0),
        );
        let fill = (area.width - 2).saturating_sub(foot_w);
        put_text(
            buf,
            area.x + 1 + foot_w,
            bottom,
            fill,
            &"─".repeat(fill as usize),
            edge,
        );
        put_text(buf, area.x + area.width - 1, bottom, 1, "╯", edge);
    }
    if area.height < 3 {
        return hits;
    }

    // The body between the borders scrolls in one-line steps.
    let body = Rect::new(area.x + 1, area.y + 1, area.width - 2, area.height - 2);
    hits.body = body;
    let heights: Vec<u16> = view.lines.iter().map(|_| 1).collect();
    let metrics = list_scroll_metrics(&heights, &[], body.height, view.scroll);
    hits.metrics = Some(metrics);
    let start = metrics.max_offset_from_bottom - metrics.offset_from_bottom;
    let show_bar = metrics.max_offset_from_bottom > 0;
    let content_width = body.width - u16::from(show_bar && body.width > 1);
    let text = Style::default().fg(palette.text);
    for (i, line) in view.lines.iter().enumerate().skip(start) {
        let y = body.y + (i - start) as u16;
        if y >= body.y + body.height {
            break;
        }
        put_text(
            buf,
            body.x,
            y,
            content_width,
            &truncate_end(line, content_width as usize),
            text,
        );
    }
    if show_bar {
        hits.scrollbar = Rect::new(body.x + body.width - 1, body.y, 1, body.height);
        render_list_scrollbar(buf, hits.scrollbar, metrics, palette);
    }
    hits
}

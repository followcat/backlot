//! The panel's renderer (U1): a `studios <n>` title, then one
//! bordered block per studio — `╭ <id> … !<n> ╮` over one-line
//! people in `├──┤`-divided groups — stacking every other block to a
//! `╶ <id> …` line when they do not all fit, scrolling under the
//! title after that. A dim shortcuts row sits last when the panel is
//! at least 8 rows tall. Pure drawing: it reads `Panel` and the
//! summary, writes cells, returns `PanelRender` (hits + whether a
//! working mark reached the screen).

use backlot_model::summary::Summary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::config::StudioKeys;
use super::draw_blocks::{
    draw_inner, draw_person, draw_person_line, draw_stacked, draw_strip, draw_top, frame_style,
};
use super::rows::{Row, Target};
use super::{Mark, Panel, rows};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::scroll::{ScrollMetrics, list_scroll_metrics, render_list_scrollbar};
use crate::chrome::text::truncate_end;

/// What the mouse needs plus the animation fact: a working mark
/// reached the frame (the loop arms the ~120 ms tick only then).
#[derive(Debug, Default)]
pub(crate) struct PanelRender {
    /// The panel's whole rect (column or full screen).
    pub area: Rect,
    /// The title row.
    pub title: Rect,
    /// The scrolled body under the title (before the scrollbar column).
    pub body: Rect,
    /// Hit rows under the title, with their targets.
    pub rows: Vec<(Rect, Target)>,
    /// The scrollbar track (empty when no overflow).
    pub scrollbar: Rect,
    /// The applied scroll metrics — drag math and max scroll.
    pub metrics: Option<ScrollMetrics>,
    /// Rows visible per page (PageUp/PageDown step).
    pub viewport_rows: usize,
    /// A working mark was drawn this frame.
    pub working_drawn: bool,
}

/// Render the panel into `area`. `body` rows under the title scroll;
/// the rightmost column carries the scrollbar on overflow; the last
/// row is the dim shortcuts line (dropped under 8 rows).
#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    buf: &mut Buffer,
    area: Rect,
    summary: Option<&Summary>,
    stage_running: bool,
    panel: &Panel,
    current: Option<&str>,
    keys: &StudioKeys,
    palette: &Palette,
) -> PanelRender {
    // Reset every cell first: `set_style` alone leaves the symbols the
    // frame underneath drew — a full-screen panel must not show pane
    // borders or titles through its blank cells.
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.reset();
            }
        }
    }
    buf.set_style(area, Style::default().bg(palette.panel_bg));
    let mut hits = PanelRender {
        area,
        ..PanelRender::default()
    };
    if area.width == 0 || area.height == 0 {
        return hits;
    }

    // `studios <n>` — accent while the panel owns the keys.
    let title = Rect::new(area.x, area.y, area.width, 1);
    let title_fg = if panel.focused {
        palette.accent
    } else {
        palette.text
    };
    let studios = summary.map(|s| s.studios.len()).unwrap_or(0);
    put_text(
        buf,
        title.x,
        title.y,
        title.width,
        &format!(" studios {studios}"),
        Style::default().fg(title_fg).add_modifier(Modifier::BOLD),
    );
    hits.title = title;

    // The last row is the dim shortcuts line — dropped under 8 rows.
    let shortcuts = area.height >= 8;
    if shortcuts {
        let keys_line = if panel.focused {
            "↑↓ move  ⏎ open  +/- priority  esc".to_string()
        } else {
            keys.shortcut_label()
        };
        put_text(
            buf,
            area.x,
            area.y + area.height - 1,
            area.width,
            &keys_line,
            Style::default().fg(palette.overlay0),
        );
    }

    // Body rows scroll under the title; the scrollbar keeps the last
    // column of the body.
    let body = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(1 + u16::from(shortcuts)),
    );
    if body.height == 0 {
        return hits;
    }
    hits.body = body;
    let all = rows(summary, stage_running, panel, current, body.height);
    // `all[0]` is the fixed Title — the scrolled list is the body under
    // it, so `panel.scroll` and the metrics index body rows only.
    let heights: Vec<u16> = all.iter().skip(1).map(|r| r.height()).collect();
    let metrics = list_scroll_metrics(&heights, &[], body.height, panel.scroll);
    let start = metrics.max_offset_from_bottom - metrics.offset_from_bottom;
    hits.metrics = Some(metrics);
    hits.viewport_rows = metrics.viewport_rows;
    let show_bar = metrics.max_offset_from_bottom > 0;
    let content_width = body.width - u16::from(show_bar && body.width > 1);

    let dim = Style::default().fg(palette.overlay0);
    let selected = panel.selected.as_ref();
    let mut y = body.y;
    for row in all.iter().skip(1 + start) {
        let height = row.height();
        if y + height > body.y + body.height {
            break; // clip: partial rows never draw
        }
        let rect = Rect::new(body.x, y, content_width, height);
        let target = row.target();
        // The selection paints the row it names — a block's `Studio`
        // target shows on its top border only.
        if selected == Some(&target)
            && matches!(
                row,
                Row::BlockTop(_)
                    | Row::Person { .. }
                    | Row::PersonLine { .. }
                    | Row::Stacked(_)
                    | Row::Strip(_)
            )
        {
            buf.set_style(rect, Style::default().bg(palette.active_row_bg));
        }
        match row {
            Row::Title => {}
            Row::StageDown => {
                put_text(buf, rect.x, rect.y, rect.width, "stage not running", dim);
            }
            Row::Empty => {
                put_text(buf, rect.x, rect.y, rect.width, "no studios", dim);
            }
            Row::BlockTop(studio) => draw_top(buf, rect, studio, current, palette),
            Row::BlockError(studio) => {
                draw_inner(buf, rect, frame_style(studio, current, palette));
                if let Some(error) = &studio.error {
                    put_text(
                        buf,
                        rect.x + 2,
                        rect.y,
                        rect.width.saturating_sub(3),
                        &truncate_end(&format!("! {error}"), rect.width.saturating_sub(3) as usize),
                        Style::default().fg(palette.red),
                    );
                }
            }
            Row::BlockNext(studio) => {
                draw_inner(buf, rect, frame_style(studio, current, palette));
                if let Some(next) = &studio.next {
                    put_text(
                        buf,
                        rect.x + 2,
                        rect.y,
                        rect.width.saturating_sub(3),
                        &truncate_end(&format!("→ {next}"), rect.width.saturating_sub(3) as usize),
                        Style::default().fg(palette.subtext0),
                    );
                }
            }
            Row::Divider(studio) => {
                let edge = frame_style(studio, current, palette);
                put_text(buf, rect.x, rect.y, 1, "├", edge);
                put_text(
                    buf,
                    rect.x + 1,
                    rect.y,
                    rect.width.saturating_sub(2),
                    &"─".repeat(rect.width.saturating_sub(2) as usize),
                    edge,
                );
                put_text(buf, rect.x + rect.width - 1, rect.y, 1, "┤", edge);
            }
            Row::BlockBottom(studio) => {
                let edge = frame_style(studio, current, palette);
                put_text(buf, rect.x, rect.y, 1, "╰", edge);
                put_text(
                    buf,
                    rect.x + 1,
                    rect.y,
                    rect.width.saturating_sub(2),
                    &"─".repeat(rect.width.saturating_sub(2) as usize),
                    edge,
                );
                put_text(buf, rect.x + rect.width - 1, rect.y, 1, "╯", edge);
            }
            Row::Person { studio, role } => {
                hits.working_drawn |= matches!(Mark::of(role), Mark::Working);
                draw_person(buf, rect, studio, role, current, panel.spin, palette);
            }
            Row::PersonLine { studio, role, kind } => {
                draw_person_line(buf, rect, studio, role, kind, current, palette);
            }
            Row::Stacked(studio) => {
                draw_stacked(buf, rect, studio, panel.spin, palette, &mut hits);
            }
            Row::Strip(studio) => {
                draw_strip(buf, rect, studio, palette);
            }
            Row::SetupHint => {
                put_text(
                    buf,
                    rect.x,
                    rect.y,
                    rect.width,
                    &truncate_end("'backlot hollywood setup'", rect.width as usize),
                    dim,
                );
            }
        }
        hits.rows.push((rect, target));
        y += height;
    }

    if show_bar {
        hits.scrollbar = Rect::new(body.x + body.width - 1, body.y, 1, body.height);
        render_list_scrollbar(buf, hits.scrollbar, metrics, palette);
    }
    hits
}

// The frame's colour and the `│` edges live in `draw_blocks` — the
// error/next/divider/bottom rows share them with the painters there.

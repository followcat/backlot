//! The panel's renderer: rows under a fixed title, scrolled by
//! `chrome/scroll`'s list metrics, with a hit map for the mouse. Pure
//! drawing — it reads `Panel` and the summary, writes cells, returns
//! `PanelRender` (hits + whether a working mark reached the screen).

use backlot_model::event::Priority;
use backlot_model::summary::Summary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::rows::{Row, Target};
use super::{Mark, Panel, mark_color, mark_symbol, rows, title_text};
use crate::chrome::palette::Palette;
use crate::chrome::scroll::{ScrollMetrics, list_scroll_metrics, render_list_scrollbar};

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

/// Render the panel into `area`. `body_height` rows under the title
/// scroll; the rightmost column carries the scrollbar on overflow.
pub(crate) fn render(
    buf: &mut Buffer,
    area: Rect,
    summary: Option<&Summary>,
    stage_running: bool,
    panel: &Panel,
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

    // Title: `working: director, actors 2` — or `idle` — cut to fit.
    let title = Rect::new(area.x, area.y, area.width, 1);
    let title_fg = if panel.focused {
        palette.accent
    } else {
        palette.text
    };
    buf.set_stringn(
        title.x,
        title.y,
        title_text(summary),
        title.width as usize,
        Style::default().fg(title_fg).add_modifier(Modifier::BOLD),
    );
    hits.title = title;

    // Body rows scroll under the title; the scrollbar keeps the last
    // column of the body.
    let body = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(1),
    );
    if body.height == 0 {
        return hits;
    }
    hits.body = body;
    let all = rows(summary, stage_running, panel);
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
        if selected == Some(&target) {
            buf.set_style(rect, Style::default().bg(palette.active_row_bg));
        }
        match row {
            Row::Title => {}
            Row::StageDown => {
                buf.set_stringn(
                    rect.x,
                    rect.y,
                    "stage not running",
                    rect.width as usize,
                    dim,
                );
            }
            Row::Empty => {
                buf.set_stringn(rect.x, rect.y, "no studios", rect.width as usize, dim);
            }
            Row::Idle => {
                buf.set_stringn(rect.x, rect.y, "idle", rect.width as usize, dim);
            }
            Row::WorkingGroup { group, working } => {
                hits.working_drawn = true;
                let mark = mark_symbol(Mark::Working, panel.spin);
                buf.set_string(
                    rect.x,
                    rect.y,
                    mark,
                    Style::default().fg(mark_color(Mark::Working, palette)),
                );
                let label = if *working > 1 {
                    format!("{group} {working}")
                } else {
                    (*group).to_owned()
                };
                buf.set_stringn(
                    rect.x + 1,
                    rect.y,
                    format!(" {label}"),
                    rect.width.saturating_sub(1) as usize,
                    Style::default().fg(palette.text),
                );
            }
            Row::Studio(studio) => {
                // The id can carry wide characters: the tail starts
                // at the position `set_stringn` reports, not a char
                // count.
                let (id_end, _) = buf.set_stringn(
                    rect.x,
                    rect.y,
                    &studio.id,
                    rect.width as usize,
                    Style::default()
                        .fg(palette.text)
                        .add_modifier(Modifier::BOLD),
                );
                if studio.state != "live" {
                    let tail = " ·finished";
                    if id_end < rect.x + rect.width {
                        buf.set_stringn(
                            id_end,
                            rect.y,
                            tail,
                            (rect.x + rect.width - id_end) as usize,
                            dim,
                        );
                    }
                }
                // The studio's stage error gets its own line, drawn
                // like a person's attention line; the first owed
                // step draws the same way under it.
                let mut y = rect.y + 1;
                if let Some(error) = &studio.error {
                    buf.set_stringn(
                        rect.x + 2,
                        y,
                        format!("! {error}"),
                        (rect.x + rect.width).saturating_sub(rect.x + 2) as usize,
                        Style::default().fg(palette.red),
                    );
                    y += 1;
                }
                if let Some(next) = &studio.next {
                    buf.set_stringn(
                        rect.x + 2,
                        y,
                        format!("→ {next}"),
                        (rect.x + rect.width).saturating_sub(rect.x + 2) as usize,
                        Style::default().fg(palette.subtext0),
                    );
                }
            }
            Row::Group {
                group,
                collapsed,
                count,
                ..
            } => {
                let arrow = if *collapsed { "▸" } else { "▾" };
                buf.set_stringn(
                    rect.x,
                    rect.y,
                    format!("{arrow} {group} {count}"),
                    rect.width as usize,
                    Style::default().fg(palette.subtext0),
                );
            }
            Row::Person { role, confirm, .. } => {
                hits.working_drawn |= matches!(Mark::of(role), Mark::Working);
                draw_person(buf, rect, role, *confirm, panel.spin, palette);
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

/// One person row: mark, name, `runner/model`, a priority flag for
/// actors — then a `doing` line, an attention line, a `last turn
/// failed` line and the armed reopen hint.
fn draw_person(
    buf: &mut Buffer,
    rect: Rect,
    role: &backlot_model::summary::RoleSummary,
    confirm: bool,
    spin: usize,
    palette: &Palette,
) {
    let mark = Mark::of(role);
    let closed = role.state == "closed";
    buf.set_string(
        rect.x,
        rect.y,
        mark_symbol(mark, spin),
        Style::default().fg(mark_color(mark, palette)),
    );
    let name_fg = if closed {
        palette.overlay0
    } else {
        palette.text
    };
    let mut x = rect.x + 2;
    // Names can carry wide characters: `set_stringn` reports the
    // display position after the name — advance by it, not a char
    // count.
    let (name_end, _) = buf.set_stringn(
        x,
        rect.y,
        &role.name,
        (rect.x + rect.width).saturating_sub(x) as usize,
        Style::default().fg(name_fg),
    );
    x = name_end;
    if let Some(priority) = role.priority {
        let (flag, fg) = match priority {
            Priority::High => ("↑", palette.red),
            Priority::Normal => ("", palette.overlay0),
            Priority::Low => ("↓", palette.overlay0),
        };
        if !flag.is_empty() && x < rect.x + rect.width {
            buf.set_stringn(
                x,
                rect.y,
                format!(" {flag}"),
                (rect.x + rect.width - x) as usize,
                Style::default().fg(fg),
            );
            x += 2;
        }
    }
    let runner = [role.runner.as_deref(), role.model.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("/");
    if !runner.is_empty() && x < rect.x + rect.width {
        buf.set_stringn(
            x,
            rect.y,
            format!(" {runner}"),
            (rect.x + rect.width - x) as usize,
            Style::default().fg(palette.overlay0),
        );
    }
    let mut y = rect.y + 1;
    if let Some(doing) = &role.doing {
        buf.set_stringn(
            rect.x + 2,
            y,
            doing,
            (rect.x + rect.width).saturating_sub(rect.x + 2) as usize,
            Style::default().fg(palette.subtext0),
        );
        y += 1;
    }
    if let Some(attention) = &role.attention {
        buf.set_stringn(
            rect.x + 2,
            y,
            format!("! {attention}"),
            (rect.x + rect.width).saturating_sub(rect.x + 2) as usize,
            Style::default().fg(palette.red),
        );
        y += 1;
    }
    // S9f: the role's last hook signal was a StopFailure — shown
    // until its next signal; it raises nothing.
    if let Some(failed) = &role.failed {
        buf.set_stringn(
            rect.x + 2,
            y,
            format!("! last turn failed: {failed}"),
            (rect.x + rect.width).saturating_sub(rect.x + 2) as usize,
            Style::default().fg(palette.peach),
        );
        y += 1;
    }
    if confirm {
        buf.set_stringn(
            rect.x + 2,
            y,
            "enter: reopen",
            (rect.x + rect.width).saturating_sub(rect.x + 2) as usize,
            Style::default().fg(palette.accent),
        );
    }
}

//! The panel's renderer (U1): a `studios <n>` title, then one
//! bordered block per studio — `╭ <id> … !<n> ╮` over one-line
//! people in `├──┤`-divided groups — stacking every other block to a
//! `╶ <id> …` line when they do not all fit, scrolling under the
//! title after that. A dim shortcuts row sits last when the panel is
//! at least 8 rows tall. Pure drawing: it reads `Panel` and the
//! summary, writes cells, returns `PanelRender` (hits + whether a
//! working mark reached the screen).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::config::StudioKeys;
use super::data::{MachineSummary, spans_machines, studio_count, studio_label};
use super::draw_blocks::{
    draw_inner, draw_person, draw_person_line, draw_stacked, draw_strip, draw_top, frame_style,
};
use super::rows::{Row, Target};
use super::{Panel, PanelMode, rows};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::scroll::{ScrollMetrics, list_scroll_metrics, render_list_scrollbar};
use crate::chrome::text::{display_width, fit_hints, truncate_end};
use crate::studio::StudioKey;
use crate::studio::glyphs::{self, Person};

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
/// row is the dim shortcuts line (dropped under 8 rows). `local` is
/// this core's display name — it labels local studios and local
/// stage lines when studios span machines.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    buf: &mut Buffer,
    area: Rect,
    machines: &[MachineSummary],
    stage_running: impl Fn(Option<&str>) -> bool,
    panel: &Panel,
    current: Option<&StudioKey>,
    local: Option<&str>,
    keys: &StudioKeys,
    sheet: Option<String>,
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

    // `lot · <n>` — accent while the panel owns the keys; the call
    // sheet's key sits at the row's right end when it fits.
    let title = Rect::new(area.x, area.y, area.width, 1);
    let title_fg = if panel.focused {
        palette.accent
    } else {
        palette.text
    };
    let studios = studio_count(machines);
    let title_text = match panel.mode {
        PanelMode::Collapsed => format!(" lot {studios}"),
        _ => format!(" lot · {studios}"),
    };
    put_text(
        buf,
        title.x,
        title.y,
        title.width,
        &title_text,
        Style::default().fg(title_fg).add_modifier(Modifier::BOLD),
    );
    if let Some(sheet) = sheet {
        let sheet_width = display_width(&sheet) as u16;
        if sheet_width
            < title
                .width
                .saturating_sub(display_width(&title_text) as u16 + 1)
        {
            put_text(
                buf,
                title.x + title.width - sheet_width,
                title.y,
                sheet_width,
                &sheet,
                Style::default().fg(palette.overlay0),
            );
        }
    }
    hits.title = title;

    // The last row is the dim shortcuts line — dropped under 8 rows.
    let shortcuts = area.height >= 8;
    if shortcuts {
        const FOCUSED_TIERS: [&str; 3] = [
            "↑↓ move  ⏎ open  +/- priority  esc",
            "↑↓ move  ⏎ open  esc",
            "esc",
        ];
        let keys_line = if panel.focused {
            fit_hints(&FOCUSED_TIERS, area.width as usize).to_string()
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
    let all = rows(machines, &stage_running, panel, current, body.height);
    let multi = spans_machines(machines);
    let label = |studio: super::data::StudioRef<'_>| {
        studio_label(&studio.studio.id, studio.machine, local, multi)
    };
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
    let selected_studio = selected.and_then(|t| t.studio().cloned());
    let frame_of = |studio: &StudioKey| {
        frame_style(
            selected_studio.as_ref() == Some(studio),
            panel.focused,
            palette,
        )
    };
    let mut y = body.y;
    for row in all.iter().skip(1 + start) {
        let height = row.height();
        if y + height > body.y + body.height {
            break; // clip: partial rows never draw
        }
        let rect = Rect::new(body.x, y, content_width, height);
        let target = row.target();
        // The selection paints the row it names — a block's `Studio`
        // target shows on its top border only — with `selection_bg`,
        // a bold `text` foreground and the accent `▌` at the row's
        // left edge (inside the block border on a person's row).
        let paints = selected == Some(&target)
            && matches!(
                row,
                Row::BlockTop(_)
                    | Row::Person { .. }
                    | Row::PersonLine { .. }
                    | Row::Stacked(_)
                    | Row::Strip(_)
            );
        if paints {
            buf.set_style(rect, glyphs::selection(palette));
        }
        match row {
            Row::Title => {}
            Row::StageDown { machine } => {
                // `host-b · stage not running` once several machines
                // answered; bare on the only machine.
                let text = match if multi { machine.or(local) } else { None } {
                    Some(name) => format!("{name} · stage not running"),
                    None => "stage not running".to_string(),
                };
                put_text(buf, rect.x, rect.y, rect.width, &text, dim);
            }
            Row::Empty => {
                put_text(buf, rect.x, rect.y, rect.width, "no studios", dim);
            }
            Row::BlockTop(studio) => {
                let frame = frame_of(&studio.key());
                draw_top(
                    buf,
                    rect,
                    *studio,
                    &label(*studio),
                    current == Some(&studio.key()),
                    frame,
                    palette,
                );
            }
            Row::BlockError(studio) => {
                draw_inner(buf, rect, frame_of(&studio.key()));
                if let Some(error) = &studio.studio.error {
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
                draw_inner(buf, rect, frame_of(&studio.key()));
                if let Some(next) = &studio.studio.next {
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
                let edge = frame_of(&studio.key());
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
                let edge = frame_of(&studio.key());
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
                hits.working_drawn |= matches!(Person::of(role), Person::Working);
                draw_person(
                    buf,
                    rect,
                    role,
                    frame_of(&studio.key()),
                    panel.spin,
                    palette,
                );
            }
            Row::PersonLine { studio, role, kind } => {
                draw_person_line(buf, rect, role, kind, frame_of(&studio.key()), palette);
            }
            Row::Stacked(studio) => {
                draw_stacked(
                    buf,
                    rect,
                    *studio,
                    &label(*studio),
                    panel.spin,
                    palette,
                    &mut hits,
                );
            }
            Row::Strip(studio) => {
                draw_strip(buf, rect, *studio, &label(*studio), palette);
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
        if paints {
            // `▌` over the row's left edge — inside the block border
            // on a person's row — after the content, so nothing
            // redraws over it.
            let bar_x = if matches!(row, Row::Person { .. } | Row::PersonLine { .. }) {
                rect.x + 1
            } else {
                rect.x
            };
            put_text(
                buf,
                bar_x,
                rect.y,
                1,
                glyphs::SELECTION_BAR,
                Style::default().bg(palette.selection_bg).fg(palette.accent),
            );
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

//! The cast's renderer: divider and ` <id>` header in the
//! agents section's slots, then the 1-line rows under `chrome/scroll`'s
//! list metrics — same scroll model as herdr's agents list.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use backlot_model::summary::RoleSummary;

use super::CastView;
use super::rows::{CastRow, cast_rows};
use crate::chrome::ClientShellConfig;
use crate::chrome::palette::Palette;
use crate::chrome::render::{ShellHitMap, put_right_text, put_text};
use crate::chrome::scroll::{list_scroll_metrics, render_list_scrollbar};
use crate::chrome::text::{display_width, truncate_end};

/// The whole cast section: `─` divider, ` <id>` + `<n>/<total>`
/// header, one blank row, then the scrolled rows.
pub(crate) fn render_cast(
    buffer: &mut Buffer,
    area: Rect,
    view: &CastView<'_>,
    config: &ClientShellConfig,
    cast_scroll: &mut usize,
    reveal_focused_agent: &mut bool,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    if area.height == 0 {
        return;
    }
    put_text(
        buffer,
        area.x,
        area.y,
        area.width,
        &"─".repeat(area.width as usize),
        Style::default().fg(palette.elevation().border_muted),
    );
    if area.height < 2 {
        return;
    }
    let header = Rect::new(area.x, area.y + 1, area.width, 1);
    match view.studio {
        Some(studio) => {
            // R3-N2: no `cast · ` prefix under a studio label — the
            // section is the studio's own; the label needs the cells.
            let id_width = header
                .width
                .saturating_sub(6) // " " plus the ` 1/12` tail
                .max(4);
            let label = view
                .label
                .as_ref()
                .map(|label| label.fit(id_width as usize))
                .unwrap_or_else(|| studio.studio.id.clone());
            let header_fg = if view.studio_focused {
                palette.text
            } else {
                palette.overlay0
            };
            put_text(
                buffer,
                header.x,
                header.y,
                header.width,
                &format!(" {label}"),
                Style::default().fg(header_fg).add_modifier(Modifier::BOLD),
            );
            put_right_text(
                buffer,
                header,
                header.y,
                &format!("{}/{}", view.index + 1, view.total),
                Style::default().fg(header_fg).add_modifier(Modifier::BOLD),
            );
        }
        // No studio yet (U2): the header alone — the `others` row is
        // the whole cast then.
        None => {
            put_text(
                buffer,
                header.x,
                header.y,
                header.width,
                " cast",
                Style::default()
                    .fg(palette.overlay0)
                    .add_modifier(Modifier::BOLD),
            );
        }
    }

    let mut rows = cast_rows(view);
    let body = Rect::new(
        area.x,
        area.y.saturating_add(3),
        area.width,
        area.height.saturating_sub(3),
    );
    hits.agent_body = body;
    if body.is_empty() || rows.is_empty() {
        *cast_scroll = 0;
        return;
    }
    if rows
        .iter()
        .any(|r| matches!(r, CastRow::ClosedDivider { .. }))
    {
        let total: usize = rows.iter().map(|row| row.lines() as usize).sum();
        if total > body.height as usize {
            rows.retain(|r| !matches!(r, CastRow::ClosedDivider { .. }));
        }
    }

    let heights: Vec<u16> = rows.iter().map(|row| row.lines()).collect();
    // The cast packs its entries: one line per person, two per shell, no gap.
    let gaps = vec![0u16; rows.len().saturating_sub(1)];
    let target = std::mem::take(reveal_focused_agent)
        .then(|| rows.iter().position(|row| row.focused()))
        .flatten();
    if let Some(target) = target {
        *cast_scroll = crate::chrome::scroll::list_scroll_start_to_reveal(
            &heights,
            &gaps,
            body.height,
            *cast_scroll,
            target,
        );
    }
    let metrics = list_scroll_metrics(&heights, &gaps, body.height, *cast_scroll);
    hits.agent_max_scroll = metrics.max_offset_from_bottom;
    hits.agent_scroll_metrics = Some(metrics);
    *cast_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_bar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = if show_bar { body.width - 1 } else { body.width };
    let mut y = body.y;
    for (index, row) in rows.iter().enumerate().skip(*cast_scroll) {
        let height = heights[index].min(body.height);
        if y.saturating_add(height) > body.bottom() {
            break;
        }
        let rect = Rect::new(body.x, y, content_width, height);
        match row {
            CastRow::Person { role, focused } => {
                if let Some(terminal) = &role.terminal {
                    hits.agents.push((rect, terminal.clone()));
                }
                draw_person(buffer, rect, role, *focused, view, palette);
            }
            CastRow::ClosedDivider { count } => {
                super::simple::draw_closed_divider(buffer, rect, *count, palette);
            }
            CastRow::Shells => super::simple::draw_shells_header(buffer, rect, palette),
            CastRow::Shell { shell, focused } => {
                hits.agents.push((rect, shell.terminal.clone()));
                draw_shell(buffer, rect, shell, *focused, view, palette);
            }
            CastRow::Others { count, expanded } => {
                hits.cast_others = rect;
                super::simple::draw_others(buffer, rect, *count, *expanded, palette);
            }
            CastRow::Hint => {
                put_text(
                    buffer,
                    rect.x.saturating_add(1),
                    rect.y,
                    rect.width.saturating_sub(1),
                    &truncate_end(
                        "select a studio (Enter on it)",
                        rect.width.saturating_sub(1) as usize,
                    ),
                    Style::default().fg(palette.overlay0),
                );
            }
            CastRow::Stopped => {
                let avail = rect.width.saturating_sub(1);
                let label = if view.server_stopped {
                    "server stopped"
                } else {
                    let full = "stage stopped — s to start";
                    if avail >= display_width(full) as u16 {
                        full
                    } else {
                        "stage stopped · s start"
                    }
                };
                put_text(
                    buffer,
                    rect.x.saturating_add(1),
                    rect.y,
                    avail,
                    &truncate_end(label, avail as usize),
                    Style::default().fg(palette.overlay0),
                );
            }
            CastRow::Agent(agent) => {
                hits.agents.push((rect, agent.pane_id.clone()));
                crate::chrome::agents::render_agent_row(buffer, rect, agent, config);
            }
        }
        y = y.saturating_add(height);
    }
    if show_bar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.agent_scrollbar = track;
        render_list_scrollbar(buffer, track, metrics, palette);
    }
}

/// A person's single-line entry: focus bar `▎`, mark, role badge,
/// priority flag, unified inline state word/age, and truncated detail.
fn draw_person(
    buffer: &mut Buffer,
    rect: Rect,
    role: &RoleSummary,
    focused: bool,
    view: &CastView<'_>,
    palette: &Palette,
) {
    if rect.height == 0 {
        return;
    }
    if focused {
        buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
        put_text(
            buffer,
            rect.x,
            rect.y,
            1,
            "▎",
            Style::default().fg(palette.overlay1),
        );
    }
    let title = role
        .terminal
        .as_deref()
        .and_then(|t| view.titles.get(t).and_then(|v| v.as_deref()));
    crate::studio::role_row::render_role_row(
        buffer,
        rect,
        rect.x + 1,
        rect.x + 3,
        rect.right(),
        role,
        view.spin,
        view.now_ms,
        title,
        palette,
    );
}

/// A shell's two rows, laid out like a person's: a dim `$` (or the
/// agent's status mark) and `shell <n>` over the terminal's title,
/// the program right-aligned on row one and the working directory's
/// last component on row two.
fn draw_shell(
    buffer: &mut Buffer,
    rect: Rect,
    shell: &super::ShellRow,
    focused: bool,
    view: &CastView<'_>,
    palette: &Palette,
) {
    if rect.height < 2 {
        return;
    }
    if focused {
        buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
        put_text(
            buffer,
            rect.x,
            rect.y,
            1,
            "▎",
            Style::default().fg(palette.overlay1),
        );
    }
    let (symbol, style) = match shell.mark {
        Some(mark) => (mark.glyph(view.spin), mark.style(palette)),
        None => ("$", Style::default().fg(palette.surface_dim)),
    };
    put_text(buffer, rect.x + 1, rect.y, 1, symbol, style);

    // Row 1: `shell <n>` left, the program right-aligned.
    let name = format!("shell {}", shell.n);
    let program_width =
        display_width(&shell.program).min(rect.width.saturating_sub(5) as usize) as u16;
    let name_width = rect
        .width
        .saturating_sub(3 + program_width + u16::from(program_width > 0));
    put_text(
        buffer,
        rect.x + 3,
        rect.y,
        name_width,
        &truncate_end(&name, name_width as usize),
        Style::default().fg(palette.text),
    );
    if program_width > 0 {
        put_text(
            buffer,
            rect.right().saturating_sub(program_width),
            rect.y,
            program_width,
            &shell.program,
            Style::default().fg(palette.overlay0),
        );
    }

    // Row 2: the terminal's title, its working directory's last
    // component at the right edge.
    let cwd_width =
        display_width(&shell.cwd_tail).min(rect.width.saturating_sub(5) as usize) as u16;
    let left_width = rect
        .width
        .saturating_sub(3 + cwd_width + u16::from(cwd_width > 0));
    put_text(
        buffer,
        rect.x + 3,
        rect.y + 1,
        left_width,
        &truncate_end(shell.title.as_deref().unwrap_or(""), left_width as usize),
        Style::default().fg(palette.subtext0),
    );
    if cwd_width > 0 {
        put_text(
            buffer,
            rect.right().saturating_sub(cwd_width),
            rect.y + 1,
            cwd_width,
            &shell.cwd_tail,
            Style::default().fg(palette.overlay0),
        );
    }
}

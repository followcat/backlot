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
    super::simple::draw_top_rule(
        buffer,
        area,
        view.region_focused,
        config.studio_chrome,
        palette,
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
            let header_style = Style::default()
                .fg(if view.region_focused {
                    palette.accent
                } else {
                    palette.text
                })
                .add_modifier(Modifier::BOLD);
            let title_prefix = if view.region_focused { " ▸ " } else { " " };
            put_text(
                buffer,
                header.x,
                header.y,
                header.width,
                &format!("{title_prefix}{label}"),
                header_style,
            );
            put_right_text(
                buffer,
                header,
                header.y,
                &format!("{}/{}", view.index + 1, view.total),
                header_style,
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
    // Under height pressure, drop non-focused roles' runner/model lines
    // first (keeping the focused role's line longest), then drop even
    // the focused role's line before scrolling or hiding the divider —
    // never hiding a role to make room for metadata.
    let fit_height = body.height as usize;
    let mut total: usize = rows.iter().map(|row| row.lines() as usize).sum();
    if total > fit_height {
        for focus_pass in [false, true] {
            for row in rows.iter_mut().rev() {
                if total <= fit_height {
                    break;
                }
                if let CastRow::Person { focused, meta, .. } = row
                    && *focused == focus_pass
                    && meta.take().is_some()
                {
                    total -= 1;
                }
            }
        }
    }
    if total > fit_height {
        rows.retain(|r| !matches!(r, CastRow::ClosedDivider { .. }));
    }

    let heights: Vec<u16> = rows.iter().map(|row| row.lines()).collect();
    // The cast packs its entries: no inter-row gap.
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
            CastRow::Person {
                role,
                focused,
                meta,
            } => {
                if let Some(terminal) = &role.terminal {
                    hits.agents.push((rect, terminal.clone()));
                }
                draw_person(buffer, rect, role, *focused, meta.as_deref(), view, palette);
                let is_cursor = view.region_focused
                    && matches!(
                        &view.cursor_target,
                        Some(crate::focus::CastTarget::Role { role: r, .. }) if *r == role.role
                    );
                let is_remembered = !view.region_focused
                    && !*focused
                    && matches!(
                        &view.remembered_target,
                        Some(crate::focus::CastTarget::Role { role: r, .. }) if *r == role.role
                    );
                if is_cursor {
                    buffer.set_style(rect, Style::default().bg(palette.selection_bg));
                    put_text(
                        buffer,
                        rect.x,
                        rect.y,
                        1,
                        "▌",
                        Style::default().fg(palette.accent).bg(palette.selection_bg),
                    );
                    if meta.is_some() && rect.height > 1 {
                        let meta_rect = Rect::new(rect.x, rect.y + 1, rect.width, 1);
                        buffer.set_style(
                            meta_rect,
                            Style::default()
                                .fg(palette.subtext0)
                                .bg(palette.selection_bg),
                        );
                    }
                    if let Some(chip_idx) = view.active_chip {
                        let chips = crate::panel::chips::row_chips_for_person(role);
                        let line1 = Rect::new(rect.x, rect.y, rect.width, 1);
                        crate::panel::chips::draw_row_chips(
                            buffer, line1, &chips, chip_idx, palette,
                        );
                    }
                } else if is_remembered {
                    put_text(
                        buffer,
                        rect.x,
                        rect.y,
                        1,
                        "·",
                        Style::default().fg(palette.overlay0),
                    );
                }
            }
            CastRow::ClosedDivider { count } => {
                super::simple::draw_closed_divider(
                    buffer,
                    rect,
                    *count,
                    config.studio_chrome,
                    palette,
                );
            }
            CastRow::Shells => super::simple::draw_shells_header(buffer, rect, palette),
            CastRow::Shell { shell, focused } => {
                hits.agents.push((rect, shell.terminal.clone()));
                draw_shell(buffer, rect, shell, *focused, view, palette);
                let is_cursor = view.region_focused
                    && matches!(
                        &view.cursor_target,
                        Some(crate::focus::CastTarget::Shell { terminal: t, .. }) if *t == shell.terminal
                    );
                if is_cursor {
                    buffer.set_style(rect, Style::default().bg(palette.selection_bg));
                    put_text(
                        buffer,
                        rect.x,
                        rect.y,
                        1,
                        "▌",
                        Style::default().fg(palette.accent).bg(palette.selection_bg),
                    );
                }
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

/// A person's entry: focus bar `▎`, mark, role badge, priority flag,
/// unified inline state word/age + detail tail on row 1, plus an indented
/// quiet `runner · model · effort` line on row 2 when room allows.
fn draw_person(
    buffer: &mut Buffer,
    rect: Rect,
    role: &RoleSummary,
    focused: bool,
    meta: Option<&str>,
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
    let line1 = Rect::new(rect.x, rect.y, rect.width, 1);
    crate::studio::role_row::render_role_row(
        buffer,
        line1,
        rect.x + 1,
        rect.x + 3,
        rect.right(),
        role,
        view.spin,
        view.now_ms,
        title,
        palette,
    );
    if let Some(meta) = meta
        && rect.height > 1
    {
        let indent = 3u16;
        let avail = rect.width.saturating_sub(indent);
        if avail > 0 {
            let clipped = truncate_end(meta, avail as usize);
            put_text(
                buffer,
                rect.x + indent,
                rect.y + 1,
                avail,
                &clipped,
                Style::default().fg(palette.subtext0),
            );
        }
    }
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

//! The cast's renderer: divider and `cast · <id>` header in the
//! agents section's slots, then the rows under `chrome/scroll`'s list
//! metrics — same scroll model as herdr's agents list, so the wheel
//! and the scrollbar take no new code.

use backlot_model::summary::RoleSummary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::CastView;
use super::rows::{CastRow, cast_rows};
use crate::chrome::ClientShellConfig;
use crate::chrome::palette::Palette;
use crate::chrome::render::{ShellHitMap, put_right_text, put_text};
use crate::chrome::scroll::{list_scroll_metrics, render_list_scrollbar};
use crate::chrome::text::{display_width, truncate_end};
use crate::studio::glyphs::Person;

/// A person's second row's state word — the shared vocabulary's own
/// word (`working`, `idle`, `slot`, `USER`, `dir`, `stalled`,
/// `failed`, `lost`, `closed`).
fn state_word(role: &RoleSummary) -> &'static str {
    Person::of(role).word()
}

/// The short model form: drop a `<provider>/` prefix and a leading
/// `claude-`, cut to at most 10 cells with `…`.
fn short_model(model: &str) -> String {
    let tail = model.rsplit('/').next().unwrap_or(model);
    truncate_end(tail.strip_prefix("claude-").unwrap_or(tail), 10)
}

/// The whole cast section: `─` divider, ` cast · <id>` + `<n>/<total>`
/// header, one blank row, then the scrolled rows.
pub(crate) fn render_cast(
    buffer: &mut Buffer,
    area: Rect,
    view: &CastView<'_>,
    config: &ClientShellConfig,
    cast_scroll: &mut usize,
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
        Style::default().fg(palette.surface_dim),
    );
    if area.height < 2 {
        return;
    }
    let header = Rect::new(area.x, area.y + 1, area.width, 1);
    match view.studio {
        Some(studio) => {
            let id_width = header
                .width
                .saturating_sub(13) // " cast · " plus the `1/12` tail
                .max(4);
            let label = view
                .label
                .as_ref()
                .map(|label| label.fit(id_width as usize))
                .unwrap_or_else(|| studio.studio.id.clone());
            put_text(
                buffer,
                header.x,
                header.y,
                header.width,
                &format!(" cast · {label}"),
                Style::default()
                    .fg(palette.overlay0)
                    .add_modifier(Modifier::BOLD),
            );
            put_right_text(
                buffer,
                header,
                header.y,
                &format!("{}/{}", view.index + 1, view.total),
                Style::default()
                    .fg(palette.overlay0)
                    .add_modifier(Modifier::BOLD),
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

    let rows = cast_rows(view);
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

    let heights: Vec<u16> = rows.iter().map(|row| row.lines()).collect();
    // The cast packs its entries: two lines per person, no gap.
    let gaps = vec![0u16; rows.len().saturating_sub(1)];
    let metrics = list_scroll_metrics(&heights, &gaps, body.height, *cast_scroll);
    hits.agent_max_scroll = metrics.max_offset_from_bottom;
    hits.agent_scroll_metrics = Some(metrics);
    *cast_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_bar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_bar));
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
            CastRow::Shells => draw_shells_header(buffer, rect, palette),
            CastRow::Shell { shell, focused } => {
                hits.agents.push((rect, shell.terminal.clone()));
                draw_shell(buffer, rect, shell, *focused, view, palette);
            }
            CastRow::Others { count, expanded } => {
                hits.cast_others = rect;
                draw_others(buffer, rect, *count, *expanded, palette);
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

/// A person's two rows: mark, `name task`, runner — over the
/// terminal's title (or its fallbacks) and the short model. Focus
/// paints the active-row background and the accent `▌`; an
/// `attention` (else a `failed` turn) replaces the whole second row,
/// model included.
fn draw_person(
    buffer: &mut Buffer,
    rect: Rect,
    role: &RoleSummary,
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
            "▌",
            Style::default().fg(palette.accent),
        );
    }
    let closed = role.state == "closed";
    let mark = Person::of(role);
    put_text(
        buffer,
        rect.x + 1,
        rect.y,
        1,
        mark.glyph(view.spin),
        mark.style(palette),
    );
    let text_fg = |main| {
        if closed { palette.overlay0 } else { main }
    };

    // Row 1: `name task` left, the runner right-aligned — the runner
    // keeps its cells while the name cuts before it.
    let mut name = role.name.clone();
    if let Some(task) = &role.task {
        name.push(' ');
        name.push_str(task);
    }
    let runner = role.runner.as_deref().unwrap_or_default();
    let runner_width = display_width(runner).min(rect.width.saturating_sub(5) as usize) as u16;
    let name_width = rect
        .width
        .saturating_sub(3 + runner_width + u16::from(runner_width > 0));
    put_text(
        buffer,
        rect.x + 3,
        rect.y,
        name_width,
        &truncate_end(&name, name_width as usize),
        Style::default().fg(text_fg(palette.text)),
    );
    if runner_width > 0 {
        put_text(
            buffer,
            rect.right().saturating_sub(runner_width),
            rect.y,
            runner_width,
            runner,
            Style::default().fg(palette.overlay0),
        );
    }

    // Row 2: `! <attention>` wins, then `last turn failed`, else the
    // title → doing → state-word fallback with the model at the right
    // edge. An alert takes the whole row: beside a model it was cut to
    // a word at the default width.
    let alert = role.attention.is_some() || role.failed.is_some();
    let model = role
        .model
        .as_deref()
        .map(short_model)
        .filter(|model| !alert && !model.is_empty());
    let model_width = model
        .as_deref()
        .map(display_width)
        .unwrap_or(0)
        .min(rect.width.saturating_sub(5) as usize) as u16;
    let left_width = rect
        .width
        .saturating_sub(3 + model_width + u16::from(model_width > 0));
    let (text, style) = if let Some(attention) = &role.attention {
        (format!("! {attention}"), Style::default().fg(palette.red))
    } else if role.failed.is_some() {
        (
            "last turn failed".to_string(),
            Style::default().fg(palette.peach),
        )
    } else {
        let title = role
            .terminal
            .as_deref()
            .and_then(|t| view.titles.get(t).and_then(|title| title.clone()))
            .or_else(|| role.doing.clone())
            .unwrap_or_else(|| state_word(role).to_string());
        (title, Style::default().fg(text_fg(palette.subtext0)))
    };
    put_text(
        buffer,
        rect.x + 3,
        rect.y + 1,
        left_width,
        &truncate_end(&text, left_width as usize),
        style,
    );
    if let Some(model) = model {
        put_text(
            buffer,
            rect.right().saturating_sub(model_width),
            rect.y + 1,
            model_width,
            &model,
            Style::default().fg(palette.overlay0),
        );
    }
}

/// The `shells` group header — a dim label line between the people
/// and their shells.
fn draw_shells_header(buffer: &mut Buffer, rect: Rect, palette: &Palette) {
    put_text(
        buffer,
        rect.x,
        rect.y,
        rect.width,
        " shells",
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::BOLD),
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
            "▌",
            Style::default().fg(palette.accent),
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

/// The `others` row: ` others <n> ▸` collapsed, `▾` expanded — the
/// in-place expansion of herdr's own agent rows.
fn draw_others(buffer: &mut Buffer, rect: Rect, count: usize, expanded: bool, palette: &Palette) {
    let arrow = if expanded { "▾" } else { "▸" };
    put_text(
        buffer,
        rect.x,
        rect.y,
        rect.width,
        &format!(" others {count} {arrow}"),
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
}

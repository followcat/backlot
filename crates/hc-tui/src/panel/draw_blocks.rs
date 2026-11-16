//! The block painters behind `draw::render`: the bordered studio
//! frame pieces, one-line people and their extra lines, the stacked
//! single-line block and the collapsed strip's studio line.
use backlot_model::event::Priority;
use backlot_model::summary::{RoleSummary, StudioSummary};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::draw::PanelRender;
use super::rows::PersonLine;
use super::{Mark, mark_color, mark_counts, mark_symbol};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};
use crate::studio;

/// The frame's colour: the current studio's block draws in the
/// accent, every other in a muted surface colour.
pub(super) fn frame_style(
    studio: &StudioSummary,
    current: Option<&str>,
    palette: &Palette,
) -> Style {
    Style::default().fg(if current == Some(studio.id.as_str()) {
        palette.accent
    } else {
        palette.surface_dim
    })
}

/// The `│` edges of a block's interior row.
pub(super) fn draw_inner(buf: &mut Buffer, rect: Rect, edge: Style) {
    put_text(buf, rect.x, rect.y, 1, "│", edge);
    if rect.width > 1 {
        put_text(buf, rect.x + rect.width - 1, rect.y, 1, "│", edge);
    }
}

/// `╭ <id> ── !<n> ╮`: the studio id sits in the top border, the
/// attention count at its right end in the blocked colour — `!` only
/// above zero.
pub(super) fn draw_top(
    buf: &mut Buffer,
    rect: Rect,
    studio: &StudioSummary,
    current: Option<&str>,
    palette: &Palette,
) {
    let frame = frame_style(studio, current, palette);
    let right = rect.x + rect.width;
    put_text(buf, rect.x, rect.y, rect.width.min(2), "╭ ", frame);
    put_text(buf, right.saturating_sub(1), rect.y, 1, "╮", frame);
    let attention = studio::attention_count(studio);
    let tail = if attention > 0 {
        format!(" !{attention} ")
    } else {
        String::new()
    };
    let tail_width = display_width(&tail) as u16;
    // `╭ <id> ── !<n> ╮`: the id keeps ≥1 dash and one space before the
    // tail or the corner.
    let id_end = right.saturating_sub(1 + tail_width + 1);
    let id_width = id_end.saturating_sub(rect.x + 2);
    let id_text = truncate_end(&studio.id, id_width as usize);
    let drawn_id_width = display_width(&id_text) as u16;
    if id_width > 0 {
        let id_style = if current == Some(studio.id.as_str()) {
            frame.add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.subtext0)
        };
        put_text(buf, rect.x + 2, rect.y, id_width, &id_text, id_style);
    }
    let fill_start = rect.x + 2 + drawn_id_width + u16::from(drawn_id_width > 0);
    let fill_end = right.saturating_sub(1 + tail_width);
    if fill_end > fill_start {
        put_text(
            buf,
            fill_start,
            rect.y,
            fill_end - fill_start,
            &"─".repeat((fill_end - fill_start) as usize),
            frame,
        );
    }
    if tail_width > 0 {
        put_text(
            buf,
            right.saturating_sub(1 + tail_width),
            rect.y,
            tail_width,
            &tail,
            Style::default().fg(palette.red),
        );
    }
}

/// One person inside a block: `│ <mark> <name> <task> <flag>   <state> │`
/// — no model, no runner, no `doing` line.
pub(super) fn draw_person(
    buf: &mut Buffer,
    rect: Rect,
    studio: &StudioSummary,
    role: &RoleSummary,
    current: Option<&str>,
    spin: usize,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame_style(studio, current, palette));
    let mark = Mark::of(role);
    let closed = role.state == "closed";
    put_text(
        buf,
        rect.x + 2,
        rect.y,
        1,
        mark_symbol(mark, spin),
        Style::default().fg(mark_color(mark, palette)),
    );
    // `name task ↑` left — the state word keeps the right edge.
    let state = mark.word();
    let state_width = display_width(state) as u16;
    let name_width = rect.width.saturating_sub(4 + state_width + 2);
    let mut name = role.name.clone();
    if let Some(task) = &role.task {
        name.push(' ');
        name.push_str(task);
    }
    if let Some(priority) = role.priority {
        let flag = match priority {
            Priority::High => " ↑",
            Priority::Normal => "",
            Priority::Low => " ↓",
        };
        name.push_str(flag);
    }
    put_text(
        buf,
        rect.x + 4,
        rect.y,
        name_width,
        &truncate_end(&name, name_width as usize),
        Style::default().fg(if closed {
            palette.overlay0
        } else {
            palette.text
        }),
    );
    put_text(
        buf,
        rect.x + rect.width.saturating_sub(1 + state_width),
        rect.y,
        state_width,
        state,
        Style::default().fg(mark_color(mark, palette)),
    );
}

/// A person's extra line at the name column: `! <attention>` wins
/// over `! last turn failed: <e>`; the armed reopen hint draws in the
/// accent.
pub(super) fn draw_person_line(
    buf: &mut Buffer,
    rect: Rect,
    studio: &StudioSummary,
    role: &RoleSummary,
    kind: &PersonLine,
    current: Option<&str>,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame_style(studio, current, palette));
    let (text, style) = match kind {
        PersonLine::Attention => (
            role.attention
                .as_deref()
                .map(|a| format!("! {a}"))
                .unwrap_or_default(),
            Style::default().fg(palette.red),
        ),
        PersonLine::Failed => (
            role.failed
                .as_deref()
                .map(|f| format!("! last turn failed: {f}"))
                .unwrap_or_default(),
            Style::default().fg(palette.peach),
        ),
        PersonLine::Confirm => (
            "enter: reopen".to_string(),
            Style::default().fg(palette.accent),
        ),
    };
    put_text(
        buf,
        rect.x + 4,
        rect.y,
        rect.width.saturating_sub(5),
        &truncate_end(&text, rect.width.saturating_sub(5) as usize),
        style,
    );
}

/// Right-aligned `(<text>, <colour>)` parts — `count` marks the
/// id's right end; returns where the tail started.
fn put_parts_right(buf: &mut Buffer, rect: Rect, parts: &[(String, Color)]) -> u16 {
    let glyphs: u16 = parts
        .iter()
        .map(|(part, _)| display_width(part) as u16)
        .sum();
    let spaces = parts.len().saturating_sub(1) as u16;
    let mut x = rect.x + rect.width.saturating_sub(glyphs + spaces);
    for (part, color) in parts {
        let w = display_width(part) as u16;
        put_text(buf, x, rect.y, w, part, Style::default().fg(*color));
        x = x.saturating_add(w + 1);
    }
    rect.x + rect.width.saturating_sub(glyphs + spaces)
}

/// A stacked block's single line: `╶ <id> ⠿2 ○3 ×1 !<n>` — each mark
/// count in its mark's colour, the attention count in the blocked
/// colour.
pub(super) fn draw_stacked(
    buf: &mut Buffer,
    rect: Rect,
    studio: &StudioSummary,
    spin: usize,
    palette: &Palette,
    hits: &mut PanelRender,
) {
    put_text(
        buf,
        rect.x,
        rect.y,
        1,
        "╶",
        Style::default().fg(palette.overlay0),
    );
    let mut parts = Vec::new();
    for (mark, n) in mark_counts(studio) {
        hits.working_drawn |= mark == Mark::Working;
        parts.push((
            format!("{}{}", mark_symbol(mark, spin), n),
            mark_color(mark, palette),
        ));
    }
    let attention = studio::attention_count(studio);
    if attention > 0 {
        parts.push((format!("!{attention}"), palette.red));
    }
    let tail = put_parts_right(buf, rect, &parts);
    let gap = u16::from(!parts.is_empty());
    let id_end = tail.saturating_sub(gap);
    if id_end > rect.x + 2 {
        let id_width = id_end - (rect.x + 2);
        put_text(
            buf,
            rect.x + 2,
            rect.y,
            id_width,
            &truncate_end(&studio.id, id_width as usize),
            Style::default().fg(palette.subtext0),
        );
    }
}

/// The collapsed strip's one line per studio: the id cut to fit, the
/// working count and `!<n>` right-aligned.
pub(super) fn draw_strip(buf: &mut Buffer, rect: Rect, studio: &StudioSummary, palette: &Palette) {
    let attention = studio::attention_count(studio);
    let working = studio::working_count(studio);
    let mut parts = Vec::new();
    if working > 0 {
        parts.push((working.to_string(), palette.yellow));
    }
    if attention > 0 {
        parts.push((format!("!{attention}"), palette.red));
    }
    let tail = put_parts_right(buf, rect, &parts);
    let gap = u16::from(!parts.is_empty());
    let id_width = tail.saturating_sub(rect.x + 1 + gap);
    if id_width > 0 {
        put_text(
            buf,
            rect.x + 1,
            rect.y,
            id_width,
            &truncate_end(&studio.id, id_width as usize),
            Style::default().fg(palette.text),
        );
    }
}

//! The block painters behind `draw::render`: the bordered studio
//! frame pieces, one-line people and their extra lines, the stacked
//! single-line block and the collapsed strip's studio line.
use backlot_model::summary::RoleSummary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::data::{StudioLabel, StudioRef};
use super::draw::PanelRender;
use super::mark_counts;
use super::rows::PersonLine;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};
use crate::studio;
use crate::studio::glyphs::{Person, PriorityMark};

/// The frame's colour: accent only while the panel owns the keys AND
/// the block owns the selected row — every block is `surface1`
/// otherwise, focused or not (A2.3, A7.2).
pub(super) fn frame_style(selected: bool, focused: bool, palette: &Palette) -> Style {
    Style::default().fg(if focused && selected {
        palette.accent
    } else {
        palette.surface1
    })
}

/// The `│` edges of a block's interior row.
pub(super) fn draw_inner(buf: &mut Buffer, rect: Rect, edge: Style) {
    put_text(buf, rect.x, rect.y, 1, "│", edge);
    if rect.width > 1 {
        put_text(buf, rect.x + rect.width - 1, rect.y, 1, "│", edge);
    }
}

/// `╭ <label> ── !<n> ╮`: the studio label sits in the top border, the
/// attention count at its right end in the blocked colour — `!` only
/// above zero. The studio whose terminal is currently focused gets a
/// subtle `▸` before its label in `text` `BOLD` — never an accent
/// border (A7.2).
pub(super) fn draw_top(
    buf: &mut Buffer,
    rect: Rect,
    studio: StudioRef<'_>,
    label: &StudioLabel,
    focused_terminal: bool,
    frame: Style,
    palette: &Palette,
) {
    let right = rect.x + rect.width;
    put_text(buf, rect.x, rect.y, rect.width.min(2), "╭ ", frame);
    put_text(buf, right.saturating_sub(1), rect.y, 1, "╮", frame);
    let attention = studio::attention_count(studio.studio);
    let tail = if attention > 0 {
        format!(" !{attention} ")
    } else {
        String::new()
    };
    let tail_width = display_width(&tail) as u16;
    // `╭ <label> ── !<n> ╮`: the label keeps ≥1 dash and one space
    // before the tail or the corner.
    let id_end = right.saturating_sub(1 + tail_width + 1);
    let id_width = id_end.saturating_sub(rect.x + 2);
    // `▸ ` before the label while its terminal is the focused pane.
    let marker_width = u16::from(focused_terminal) * 2;
    let id_text = label.fit(id_width.saturating_sub(marker_width) as usize);
    let drawn_id_width = display_width(&id_text) as u16;
    if focused_terminal && id_width >= 2 {
        put_text(
            buf,
            rect.x + 2,
            rect.y,
            1,
            crate::studio::glyphs::FOCUSED_STUDIO,
            Style::default()
                .fg(palette.text)
                .add_modifier(Modifier::BOLD),
        );
    }
    if id_width > marker_width {
        let id_style = if focused_terminal {
            Style::default().fg(palette.text)
        } else {
            Style::default().fg(palette.subtext0)
        };
        put_text(
            buf,
            rect.x + 2 + marker_width,
            rect.y,
            id_width - marker_width,
            &id_text,
            id_style,
        );
    }
    let fill_start = rect.x + 2 + marker_width + drawn_id_width + u16::from(drawn_id_width > 0);
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
/// — no model, no runner, no `doing` line. The mark, the state word
/// and the priority flag all read the shared vocabulary.
pub(super) fn draw_person(
    buf: &mut Buffer,
    rect: Rect,
    role: &RoleSummary,
    frame: Style,
    spin: usize,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame);
    let mark = Person::of(role);
    let closed = role.state == "closed";
    put_text(
        buf,
        rect.x + 2,
        rect.y,
        1,
        mark.glyph(spin),
        mark.style(palette),
    );
    // `name task ↑` left — the state word keeps the right edge.
    let state = mark.word();
    let state_width = display_width(state) as u16;
    let name_width = rect.width.saturating_sub(4 + state_width + 2);
    let flag = role.priority.and_then(PriorityMark::of);
    let flag_width = flag.map_or(0, |f| display_width(f.glyph()) + 1) as u16;
    let mut name = role.name.clone();
    if let Some(task) = &role.task {
        name.push(' ');
        name.push_str(task);
    }
    let name_text = truncate_end(&name, name_width.saturating_sub(flag_width) as usize);
    let drawn_name_width = display_width(&name_text) as u16;
    put_text(
        buf,
        rect.x + 4,
        rect.y,
        name_width.saturating_sub(flag_width),
        &name_text,
        Style::default().fg(if closed {
            palette.overlay0
        } else {
            palette.text
        }),
    );
    if let Some(flag) = flag {
        put_text(
            buf,
            rect.x + 4 + drawn_name_width + 1,
            rect.y,
            1,
            flag.glyph(),
            Style::default().fg(flag.color(palette)),
        );
    }
    put_text(
        buf,
        rect.x + rect.width.saturating_sub(1 + state_width),
        rect.y,
        state_width,
        state,
        mark.style(palette),
    );
}

/// A person's extra line at the name column: `! <attention>` wins
/// over `! last turn failed: <e>`; the armed reopen hint draws in the
/// accent. The attention line takes its owner's glyph colour — a
/// user's prompt red, the director's peach.
pub(super) fn draw_person_line(
    buf: &mut Buffer,
    rect: Rect,
    role: &RoleSummary,
    kind: &PersonLine,
    frame: Style,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame);
    let (text, style) = match kind {
        PersonLine::Attention => (
            role.attention
                .as_deref()
                .map(|a| format!("! {a}"))
                .unwrap_or_default(),
            Person::of(role).style(palette),
        ),
        PersonLine::Failed => (
            role.failed
                .as_deref()
                .map(|f| format!("! last turn failed: {f}"))
                .unwrap_or_default(),
            Person::Failed.style(palette),
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
/// label's right end; returns where the tail started.
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

/// A stacked block's single line: `╶ <label> ⠿2 ○3 ×1 !<n>` — each
/// mark count in its mark's colour, the attention count in the
/// blocked colour.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_stacked(
    buf: &mut Buffer,
    rect: Rect,
    studio: StudioRef<'_>,
    label: &StudioLabel,
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
    for (mark, n) in mark_counts(studio.studio) {
        hits.working_drawn |= mark == Person::Working;
        parts.push((format!("{}{}", mark.glyph(spin), n), mark.color(palette)));
    }
    let attention = studio::attention_count(studio.studio);
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
            &label.fit(id_width as usize),
            Style::default().fg(palette.subtext0),
        );
    }
}

/// The collapsed strip's one line per studio: the label cut to fit,
/// the working count and `!<n>` right-aligned.
pub(super) fn draw_strip(
    buf: &mut Buffer,
    rect: Rect,
    studio: StudioRef<'_>,
    label: &StudioLabel,
    palette: &Palette,
) {
    let attention = studio::attention_count(studio.studio);
    let working = studio::working_count(studio.studio);
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
            &label.fit(id_width as usize),
            Style::default().fg(palette.text),
        );
    }
}

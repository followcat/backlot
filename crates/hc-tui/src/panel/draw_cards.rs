//! The block's card painters behind `draw::render`: the stacked
//! single-line block and the collapsed strip's header and role
//! rows — the never-blank 16-column edge.

use backlot_model::summary::RoleSummary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

use super::data::{StudioLabel, StudioRef};
use super::draw::PanelRender;
use super::mark_counts;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};
use crate::studio;
use crate::studio::glyphs::{Health, Person, PriorityMark};

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

/// The collapsed strip's studio line: `● acme·Loc  1/3` — the
/// health glyph, a short `id·<machine>` label and `<done>/<total>`
/// or `!<n>` right-aligned (A4.3). Never blank: a studio with no
/// tasks and nothing waiting still shows its health and name.
/// The collapsed strip's studio line: `● acme·Loc  1/3` — the
/// health glyph, a short `id·<machine>` label and `<done>/<total>`
/// or `!<n>` right-aligned (A4.3). Never blank: a studio with no
/// tasks and nothing waiting still shows its health and name.
pub(super) fn draw_strip(
    buf: &mut Buffer,
    rect: Rect,
    studio: StudioRef<'_>,
    health: Health,
    local: Option<&str>,
    palette: &Palette,
) {
    put_text(
        buf,
        rect.x,
        rect.y,
        1,
        health.glyph(),
        Style::default().fg(health.color(palette)),
    );
    let waiting = studio.studio.waiting;
    let mut parts = Vec::new();
    if waiting > 0 {
        let user = studio.studio.roles.iter().any(|r| r.attention_user);
        parts.push((
            format!("!{waiting}"),
            if user { palette.red } else { palette.peach },
        ));
    }
    if studio.studio.tasks_total > 0 {
        parts.push((
            format!("{}/{}", studio.studio.tasks_done, studio.studio.tasks_total),
            palette.overlay0,
        ));
    }
    let tail = put_parts_right(buf, rect, &parts);
    let gap = u16::from(!parts.is_empty());
    // `<short_id>·<machine 2-3>` — the machine keeps its three
    // chars, the id truncates first (A4.3).
    let machine = studio.machine.or(local).unwrap_or("local");
    let short_m: String = machine.chars().take(3).collect();
    let label_w = tail.saturating_sub(rect.x + 2 + gap);
    let id_room = (label_w as usize).saturating_sub(1 + short_m.chars().count());
    let id = truncate_end(&studio.studio.id, id_room.max(1));
    let label = format!("{id}·{short_m}");
    if label_w > 0 {
        put_text(
            buf,
            rect.x + 2,
            rect.y,
            label_w,
            &truncate_end(&label, label_w as usize),
            Style::default().fg(palette.text),
        );
    }
}

/// A strip's active-or-blocked role line: `× dir      USER` or
/// `◐ act-a    t1 ↑` — the mark keeps its colour, the name
/// truncates, the task id and its priority flag or the `USER` word
/// hold the right edge (A4.3).
/// A strip's active-or-blocked role line: `× dir      USER` or
/// `◐ act-a    t1 ↑` — the mark keeps its colour, the name
/// truncates, the task id and its priority flag or the `USER` word
/// hold the right edge (A4.3).
pub(super) fn draw_strip_role(buf: &mut Buffer, rect: Rect, role: &RoleSummary, palette: &Palette) {
    let mark = Person::of(role);
    put_text(
        buf,
        rect.x + 1,
        rect.y,
        1,
        mark.glyph(0),
        mark.style(palette),
    );
    let flag = role.priority.and_then(PriorityMark::of);
    let mut right = String::new();
    if mark == Person::UserPrompt {
        right = "USER".to_string();
    } else if let Some(task) = &role.task {
        right = task.clone();
        if let Some(flag) = flag {
            right.push(' ');
            right.push_str(flag.glyph());
        }
    } else if mark == Person::DirPrompt {
        right = "dir".to_string();
    }
    let right_w = display_width(&right) as u16;
    if right_w > 0 {
        put_text(
            buf,
            rect.x + rect.width.saturating_sub(right_w),
            rect.y,
            right_w,
            &right,
            Style::default().fg(if mark == Person::UserPrompt {
                palette.red
            } else {
                palette.subtext0
            }),
        );
    }
    let name_w = rect
        .width
        .saturating_sub(3 + right_w + u16::from(right_w > 0));
    put_text(
        buf,
        rect.x + 3,
        rect.y,
        name_w,
        &truncate_end(&role.name, name_w as usize),
        Style::default().fg(palette.subtext0),
    );
}

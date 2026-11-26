//! The block's card painters behind `draw::render`: the stacked
//! single-line block and the collapsed strip's header and role
//! rows — the never-blank 16-column edge.

use backlot_model::summary::RoleSummary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

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

fn studio_title_style(
    selected: bool,
    focused: bool,
    unselected_fg: Color,
    palette: &Palette,
) -> Style {
    if selected {
        Style::default()
            .fg(if focused {
                palette.accent
            } else {
                palette.text
            })
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(unselected_fg)
    }
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
    selected: bool,
    focused: bool,
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
        let style = studio_title_style(selected, focused, palette.subtext0, palette);
        put_text(
            buf,
            rect.x + 2,
            rect.y,
            id_width,
            &label.fit(id_width as usize),
            style,
        );
    }
}

/// The collapsed strip's studio line: `● acme·Loc  1/3` — the
/// health glyph, a short `id·<machine>` label and `<done>/<total>`
/// or `!<n>` right-aligned (A4.3). Never blank: a studio with no
/// tasks and nothing waiting still shows its health and name.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_strip(
    buf: &mut Buffer,
    rect: Rect,
    studio: StudioRef<'_>,
    health: Health,
    selected: bool,
    focused: bool,
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
    let waiting = crate::studio::waiting_count(studio.studio);
    let mut parts = Vec::new();
    if waiting > 0 {
        let user = crate::studio::waiting_is_user(studio.studio);
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
        let style = studio_title_style(selected, focused, palette.text, palette);
        put_text(
            buf,
            rect.x + 2,
            rect.y,
            label_w,
            &truncate_end(&label, label_w as usize),
            style,
        );
    }
}

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
        // R3-N2: `t2 dir` — a director-blocked person is not just
        // its task id; the `dir` cue stays beside it.
        if mark == Person::DirPrompt {
            right.push_str(" dir");
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

/// R3-N9: compact 1-line calm/owed summary under an idle strip studio
/// (`○2 ✓1` or a short owed cue), fitting inside `rect.width - 2`
/// (`14` inner columns at width 16).
pub(super) fn draw_strip_summary(
    buf: &mut Buffer,
    rect: Rect,
    studio: StudioRef<'_>,
    stage: bool,
    reachable: bool,
    palette: &Palette,
) {
    let start_x = rect.x + 2;
    let inner_w = rect.width.saturating_sub(2);
    if inner_w == 0 {
        return;
    }
    if !reachable {
        put_text(
            buf,
            start_x,
            rect.y,
            inner_w,
            &truncate_end("× offline", inner_w as usize),
            Style::default().fg(palette.red),
        );
        return;
    }
    if !stage {
        put_text(
            buf,
            start_x,
            rect.y,
            inner_w,
            &truncate_end("◌ stage down", inner_w as usize),
            Style::default().fg(palette.peach),
        );
        return;
    }
    if studio.studio.error.is_some() {
        put_text(
            buf,
            start_x,
            rect.y,
            inner_w,
            &truncate_end("! error", inner_w as usize),
            Style::default().fg(palette.red),
        );
        return;
    }
    if studio.studio.ready_to_finish {
        put_text(
            buf,
            start_x,
            rect.y,
            inner_w,
            &truncate_end("✓ ready", inner_w as usize),
            Style::default().fg(palette.teal),
        );
        return;
    }
    let counts = mark_counts(studio.studio);
    if counts.is_empty() {
        put_text(
            buf,
            start_x,
            rect.y,
            inner_w,
            &truncate_end("○ idle", inner_w as usize),
            Style::default().fg(palette.overlay0),
        );
        return;
    }
    let end_x = rect.x + rect.width;
    let mut x = start_x;
    for (i, (mark, n)) in counts.into_iter().enumerate() {
        let token = format!("{}{}", mark.glyph(0), n);
        let w = display_width(&token) as u16;
        let gap = u16::from(i > 0);
        if x + gap + w > end_x {
            break;
        }
        x += gap;
        put_text(
            buf,
            x,
            rect.y,
            w,
            &token,
            Style::default().fg(mark.color(palette)),
        );
        x += w;
    }
}

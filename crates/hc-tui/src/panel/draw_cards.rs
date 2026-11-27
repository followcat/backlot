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
use crate::chrome::render::{put_role_badged_text, put_text};
use crate::chrome::text::{display_width, truncate_end};
use crate::studio;
use crate::studio::glyphs::{self, Health, Person, PriorityMark};

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

fn studio_title_style(selected: bool, focused: bool, palette: &Palette) -> Style {
    let fg = if selected && focused {
        palette.accent
    } else {
        palette.text
    };
    Style::default().fg(fg).add_modifier(Modifier::BOLD)
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
    focused_terminal: bool,
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
    let capsule = focused_terminal && !(selected && focused);
    let cap_pad = u16::from(capsule);
    if id_end > rect.x + 2 + cap_pad {
        let id_width = id_end - (rect.x + 2 + cap_pad);
        let fitted = label.fit(id_width as usize);
        if capsule {
            let cap_style = glyphs::studio_capsule_style(palette);
            let drawn = display_width(&fitted) as u16;
            put_text(buf, rect.x + 1, rect.y, 1, " ", cap_style);
            put_text(buf, rect.x + 2, rect.y, id_width, &fitted, cap_style);
            if drawn > 0 && rect.x + 2 + drawn < id_end {
                put_text(buf, rect.x + 2 + drawn, rect.y, 1, " ", cap_style);
            }
        } else {
            let style = studio_title_style(selected, focused, palette);
            put_text(buf, rect.x + 2, rect.y, id_width, &fitted, style);
        }
    }
}

/// The collapsed strip's studio line: `▸ beta    0/0` or `✓ gamm    5/5` —
/// the focus/health glyph, studio id (plus `·<machine>` only when studios span
/// machines), and `<done>/<total>` or `!<n>` right-aligned.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_strip(
    buf: &mut Buffer,
    rect: Rect,
    studio: StudioRef<'_>,
    health: Health,
    selected: bool,
    focused: bool,
    focused_terminal: bool,
    multi: bool,
    local: Option<&str>,
    palette: &Palette,
) {
    let (glyph, glyph_fg) = if focused_terminal {
        (crate::studio::glyphs::FOCUSED_STUDIO, palette.accent)
    } else {
        (health.glyph(), health.color(palette))
    };
    put_text(buf, rect.x, rect.y, 1, glyph, Style::default().fg(glyph_fg));
    let waiting = crate::studio::waiting_count(studio.studio);
    let mut parts = Vec::new();
    if waiting > 0 {
        let user = crate::studio::waiting_is_user(studio.studio);
        parts.push((
            format!("!{waiting}"),
            if user { palette.red } else { palette.peach },
        ));
    }
    let has_design = super::draw_design::active_design(studio.studio).is_some();
    if studio.studio.tasks_total > 0 || (focused_terminal && !has_design) {
        parts.push((
            format!("{}/{}", studio.studio.tasks_done, studio.studio.tasks_total),
            palette.overlay0,
        ));
    }
    if let Some((badge, color)) = super::draw_design::strip_badge(
        studio.studio,
        rect.width.saturating_sub(2) as usize,
        palette,
    ) {
        parts.push((badge, color));
    }
    let tail = put_parts_right(buf, rect, &parts);
    let gap = u16::from(!parts.is_empty());
    let label_w = tail.saturating_sub(rect.x + 2 + gap);
    let label = if multi {
        let machine = studio.machine.or(local).unwrap_or("local");
        let short_m: String = machine.chars().take(3).collect();
        let id_room = (label_w as usize).saturating_sub(1 + short_m.chars().count());
        let id = truncate_end(&studio.studio.id, id_room.max(1));
        format!("{id}·{short_m}")
    } else {
        truncate_end(&studio.studio.id, label_w as usize)
    };
    if label_w > 0 {
        let style = studio_title_style(selected, focused, palette);
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

/// A strip's active-or-blocked role line: `× Ⓐ t8   USER` or
/// `◐ Ⓐ t9   work` — the mark and badge keep their colour, the label
/// truncates, and the short state word holds the right edge.
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
    let right = match mark {
        Person::UserPrompt => "USER",
        Person::DirPrompt => "dir",
        Person::Asks => "asks",
        Person::NeedsYou => "needs",
        Person::Stuck => "stuck",
        Person::Failed => "failed",
        Person::Lost => "lost",
        Person::Working => "work",
        Person::Slot => "queue",
        _ => "",
    };
    let right_w = display_width(right) as u16;
    if right_w > 0 {
        put_text(
            buf,
            rect.x + rect.width.saturating_sub(right_w),
            rect.y,
            right_w,
            right,
            match mark {
                Person::UserPrompt => Style::default().fg(palette.red),
                Person::Working => Style::default().fg(palette.subtext0),
                _ => mark.style(palette),
            },
        );
    }
    let flag = role.priority.and_then(PriorityMark::of);
    let mut label = glyphs::role_label(role);
    if let Some(flag) = flag {
        label.push(' ');
        label.push_str(flag.glyph());
    }
    let name_w = rect
        .width
        .saturating_sub(3 + right_w + u16::from(right_w > 0));
    put_role_badged_text(
        buf,
        rect.x + 3,
        rect.y,
        name_w,
        &truncate_end(&label, name_w as usize),
        Style::default().fg(palette.subtext0),
        palette,
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
    if studio.studio.needs_upgrade {
        put_text(
            buf,
            start_x,
            rect.y,
            inner_w,
            &truncate_end("! upgrade", inner_w as usize),
            Style::default().fg(palette.peach),
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

//! The band's top title row and bottom perforation edge:
//! `✦  H O L L Y W O O D   ● live · <agent>` on the left (compressed to
//! `✦ HOLLYWOOD` under 100 columns), and Lot-pill-matching bucket counts
//! (`6 studios · !B · ?A · Cc · ✓R ready`) on the right.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::Target;
use super::band::MARGIN;
use super::draw::{HollywoodRender, MARK};
use super::items::WaitBucket;
use super::rows;
use super::sheet::Wait;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::display_width;
use crate::panel::data::{MachineSummary, studio_count};

/// Compressed mark shown below 100 columns.
const NARROW_MARK: &str = "✦ HOLLYWOOD";

/// One perforation row (`▀▀` every four columns for the bottom edge).
pub(crate) fn draw_sprocket(buf: &mut Buffer, area: Rect, y: u16, glyph: char, palette: &Palette) {
    let elev = palette.elevation();
    let (fg, bg) = if elev.is_terminal_16 {
        (elev.border_muted, Color::Reset)
    } else if elev.is_dark {
        (elev.border_muted, palette.panel_bg)
    } else {
        (elev.border_muted, elev.surface)
    };
    buf.set_style(Rect::new(area.x, y, area.width, 1), Style::default().bg(bg));
    let style = Style::default().fg(fg).bg(bg);
    let hole = format!("{glyph}{glyph}");
    let mut x = area.x + 1;
    while x + 1 < area.x + area.width.saturating_sub(1) {
        put_text(buf, x, y, 2, &hole, style);
        x += 4;
    }
}

fn width_of(text: &str) -> u16 {
    display_width(text) as u16
}

fn build_right_segs(
    studios: usize,
    waits: &[Wait<'_>],
    title_scroll: Option<(usize, usize)>,
    compact: bool,
    palette: &Palette,
) -> Vec<(String, Style)> {
    let elev = palette.elevation();
    let base = Style::default().bg(elev.surface);
    let muted = base.fg(elev.muted_surface);
    let (mut b, mut a, mut c, mut r) = (0usize, 0usize, 0usize, 0usize);
    for w in waits {
        match WaitBucket::of(w) {
            WaitBucket::Blocking => b += 1,
            WaitBucket::Asks => a += 1,
            WaitBucket::Cards => c += 1,
            WaitBucket::Ready => r += 1,
        }
    }
    let mut segs = Vec::new();
    let studio_text = if compact {
        format!("{studios}")
    } else {
        format!("{studios} studio{}", if studios == 1 { "" } else { "s" })
    };
    segs.push((studio_text, muted));
    let mut push_bucket = |tok: String, fg: Color| {
        segs.push((" · ".to_string(), muted));
        segs.push((tok, base.fg(fg).add_modifier(Modifier::BOLD)));
    };
    if b > 0 {
        push_bucket(format!("!{b}"), palette.red);
    }
    if a > 0 {
        push_bucket(format!("?{a}"), palette.peach);
    }
    if c > 0 {
        push_bucket(format!("{c}c"), palette.peach);
    }
    if r > 0 {
        let tok = if compact {
            format!("✓{r}")
        } else {
            format!("✓{r} ready")
        };
        push_bucket(tok, palette.teal);
    }
    if let Some((top, bot)) = title_scroll {
        segs.push((" · ".to_string(), muted));
        segs.push((
            format!("{top}–{bot}/{}", waits.len()),
            muted.add_modifier(Modifier::BOLD),
        ));
    }
    segs
}

fn segs_width(segs: &[(String, Style)]) -> u16 {
    segs.iter().map(|(s, _)| width_of(s)).sum()
}

/// The title row: left mark + agent status, right-aligned Lot-matching
/// studio and wait bucket summary (`!B`, `?A`, `Cc`, `✓R ready`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_title(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    machines: &[MachineSummary],
    waits: &[Wait<'_>],
    agent: bool,
    activity: rows::HollywoodActivity,
    spin: usize,
    title_scroll: Option<(usize, usize)>,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    let row = Rect::new(area.x, y, area.width, 1);
    hits.title = row;
    hits.rows.push((row, Target::Title));
    let elev = palette.elevation();
    let base = Style::default().bg(elev.surface);

    let mark = if area.width < 100 { NARROW_MARK } else { MARK };
    let mark_w = mark.chars().count() as u16;
    let mark_gap = if area.width < 100 { 2 } else { 3 };
    put_text(
        buf,
        area.x + MARGIN,
        y,
        mark_w,
        mark,
        base.fg(palette.accent).add_modifier(Modifier::BOLD),
    );
    let x = area.x + MARGIN + mark_w + mark_gap;

    let studios = studio_count(machines);
    let full_right = build_right_segs(studios, waits, title_scroll, false, palette);
    let compact_right = build_right_segs(studios, waits, title_scroll, true, palette);

    let summary = rows::local_summary(machines);
    let hw = summary.and_then(|s| s.hollywood.as_ref());
    let state = hw.map(|h| h.state.as_str());
    let (agent_line, short_line) = if let Some(hw) = hw
        && let Some(over) =
            super::handoff_ui::title_override(hw, activity, crate::update_state::now_ms())
    {
        over
    } else if agent {
        let state = state.unwrap_or("down");
        let suffix = match activity {
            rows::HollywoodActivity::Working => " · thinking…",
            rows::HollywoodActivity::Queued => " · queued…",
            rows::HollywoodActivity::Idle => "",
        };
        let short = match activity {
            rows::HollywoodActivity::Working => "thinking…".to_string(),
            rows::HollywoodActivity::Queued => "queued…".to_string(),
            rows::HollywoodActivity::Idle => state.to_string(),
        };
        let details = summary
            .and_then(|s| s.hollywood.as_ref())
            .map(|h| {
                [h.runner.as_deref(), h.model.as_deref(), h.effort.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .unwrap_or_default();
        if details.is_empty() {
            (format!("{state}{suffix}"), short)
        } else {
            (format!("{state} · {details}{suffix}"), short)
        }
    } else {
        ("no agent".to_string(), String::new())
    };

    let pick_for = |right_w: u16| -> Option<String> {
        let right_x = if right_w > 0 {
            (area.x + area.width).saturating_sub(MARGIN + right_w)
        } else {
            area.x + area.width
        };
        let room = right_x.saturating_sub(x + 2);
        if width_of(&agent_line) <= room {
            Some(agent_line.clone())
        } else if !short_line.is_empty() && width_of(&short_line) <= room {
            Some(short_line.clone())
        } else {
            None
        }
    };

    let (chosen_right, text) = {
        let full_w = segs_width(&full_right);
        let compact_w = segs_width(&compact_right);
        if let Some(t) = pick_for(full_w) {
            (full_right, t)
        } else if let Some(t) = pick_for(compact_w) {
            (compact_right, t)
        } else if let Some(t) = pick_for(0) {
            (Vec::new(), t)
        } else {
            (compact_right, String::new())
        }
    };

    if agent {
        let (glyph, colour) = if let Some(hw) = hw
            && super::handoff_ui::handoff_phase(hw) == Some("needs_approval")
        {
            ("⚠", palette.red)
        } else if let Some(hw) = hw
            && super::handoff_ui::is_quota_or_error(hw)
            && !super::handoff_ui::is_handoff_active(hw)
        {
            ("●", palette.red)
        } else {
            match activity {
                rows::HollywoodActivity::Working => (
                    crate::studio::glyphs::Person::Working.glyph(spin),
                    palette.accent,
                ),
                rows::HollywoodActivity::Queued => (
                    crate::studio::glyphs::Person::Working.glyph(spin),
                    palette.peach,
                ),
                rows::HollywoodActivity::Idle => {
                    ("●", rows::state_color(state.unwrap_or("down"), palette))
                }
            }
        };
        if x < area.x + area.width {
            put_text(buf, x, y, 1, glyph, base.fg(colour));
        }
    } else if x < area.x + area.width {
        put_text(buf, x, y, 1, "·", base.fg(elev.muted_surface));
    }

    let right_w = segs_width(&chosen_right);
    let right_x = (area.x + area.width).saturating_sub(MARGIN + right_w);
    let text_room = right_x.saturating_sub(x + 2);
    if !text.is_empty() {
        put_text(
            buf,
            x + 2,
            y,
            text_room,
            &text,
            base.fg(if agent {
                palette.subtext0
            } else {
                elev.muted_surface
            }),
        );
    }
    if right_w > 0 && right_x > x {
        let mut rx = right_x;
        for (seg, style) in chosen_right {
            let sw = width_of(&seg);
            put_text(buf, rx, y, sw, &seg, style);
            rx += sw;
        }
    }
}

//! The band's top and bottom edges: the sprocket rows and the
//! title row — the mark, the agent's square and role, and the
//! right-aligned studio count with what waits on you.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::Target;
use super::band::MARGIN;
use super::draw::{HollywoodRender, MARK};
use super::rows;
use super::sheet::Wait;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::panel::data::{MachineSummary, studio_count};

/// One sprocket row: `▄▄`/`▀▀` every four columns. On `sidebar_bg`
/// the holes are `surface1`; where `sidebar_bg` is `Reset` (light
/// themes) they are `overlay0` on `surface0` so the perforations
/// stay opaque and legible.
pub(crate) fn draw_sprocket(buf: &mut Buffer, area: Rect, y: u16, glyph: char, palette: &Palette) {
    let (fg, bg) = if palette.sidebar_bg != Color::Reset {
        (palette.surface1, palette.sidebar_bg)
    } else {
        (palette.overlay0, palette.surface0)
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

/// The display width of `text`, in cells — a floor for the title row's
/// room sums.
fn width_of(text: &str) -> u16 {
    crate::chrome::text::display_width(text) as u16
}

/// The title row: the mark, the agent's square and role, and the
/// right-aligned studio count with what waits on you — red when the
/// user owns any of it, peach when only cards and finish-ready
/// studios do. The count spans machines; the agent's words stay
/// this core's.
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
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    let row = Rect::new(area.x, y, area.width, 1);
    hits.title = row;
    hits.rows.push((row, Target::Title));
    let base = Style::default().bg(palette.surface_dim);
    let mark_w = MARK.chars().count() as u16;
    put_text(
        buf,
        area.x + MARGIN,
        y,
        mark_w,
        MARK,
        base.fg(palette.accent).add_modifier(Modifier::BOLD),
    );
    let x = area.x + MARGIN + mark_w + 3;

    let studios = studio_count(machines);
    let count = format!("{studios} studio{}", if studios == 1 { "" } else { "s" });
    let wait = if waits.is_empty() {
        String::new()
    } else {
        format!("  ·  {} waiting on you", waits.len())
    };
    // User urgency is red: a prompt or a down/offline stage. Cards
    // and ready studios alone are peach.
    let user_urgent = waits
        .iter()
        .any(|w| matches!(w, Wait::Prompt(_) | Wait::Stage(_)));
    let summary = rows::local_summary(machines);
    let hw = summary.and_then(|s| s.hollywood.as_ref());
    let state = hw.map(|h| h.state.as_str());
    // The agent's words: the full line when it fits, the state alone
    // when it does not. If even that is squeezed, the counts step
    // aside — the state is what the row must say.
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
    let mut right_w = count.chars().count() as u16 + wait.chars().count() as u16;
    let mut right_x = (area.x + area.width).saturating_sub(MARGIN + right_w);
    let mut room = right_x.saturating_sub(x + 2);
    // The longest line that fits; if none does, the counts step aside
    // for it — the state is what the row must say.
    let pick = |room: u16| {
        if width_of(&agent_line) <= room {
            agent_line.clone()
        } else if !short_line.is_empty() && width_of(&short_line) <= room {
            short_line.clone()
        } else {
            String::new()
        }
    };
    let mut text = pick(room);
    if text.is_empty() && right_w > 0 {
        right_w = 0;
        right_x = area.x + area.width;
        room = right_x.saturating_sub(x + 2);
        text = pick(room);
    }
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
        put_text(buf, x, y, 1, glyph, base.fg(colour));
    } else {
        put_text(buf, x, y, 1, "·", base.fg(palette.overlay0));
    }
    if !text.is_empty() {
        put_text(
            buf,
            x + 2,
            y,
            room,
            &text,
            base.fg(if agent {
                palette.overlay1
            } else {
                palette.overlay0
            }),
        );
    }
    if right_w > 0 && right_x > x {
        put_text(
            buf,
            right_x,
            y,
            count.chars().count() as u16,
            &count,
            base.fg(palette.overlay0),
        );
        if !wait.is_empty() {
            let style = base
                .fg(if user_urgent {
                    palette.red
                } else {
                    palette.peach
                })
                .add_modifier(Modifier::BOLD);
            put_text(
                buf,
                right_x + count.chars().count() as u16,
                y,
                wait.chars().count() as u16,
                &wait,
                style,
            );
        }
    }
}

//! The collapsed bar: one row at the right end of the top row, sized
//! to its content and capped at a third of the frame, on
//! `surface_dim`. `✦ hollywood  ■ 3 studios  !1 ` — the mark, the
//! agent's square in its role state, the studio count and the waiting
//! chip. A click on it expands the bar.

use backlot_model::summary::Summary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::Target;
use super::draw::HollywoodRender;
use super::rows;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::truncate_end;

/// The strip's width cap: a third of the frame.
const STRIP_FRAC: u16 = 3;

/// Draw the strip at the top right of `frame`.
pub(crate) fn render(
    buf: &mut Buffer,
    frame: Rect,
    summary: Option<&Summary>,
    agent: bool,
    palette: &Palette,
) -> HollywoodRender {
    if frame.width == 0 || frame.height == 0 {
        return HollywoodRender::default();
    }
    let cap = (frame.width / STRIP_FRAC).max(1);
    let base = Style::default().bg(palette.surface_dim);
    let content = rows::strip(summary, agent);
    // Pieces in draw order — the same text `rows::strip_text` builds:
    // the mark, the agent's square, the count and the waiting chip.
    let mut pieces: Vec<(String, Style)> = vec![
        ("✦ hollywood".into(), base.fg(palette.accent)),
        (
            "  ■".into(),
            base.fg(match content.state {
                Some(state) => state_color(state, palette),
                None => palette.overlay0,
            }),
        ),
        (
            format!(
                " {} studio{}",
                content.studios,
                if content.studios == 1 { "" } else { "s" }
            ),
            base.fg(palette.text),
        ),
    ];
    if content.waiting > 0 {
        pieces.push((
            format!("  !{} ", content.waiting),
            base.fg(palette.panel_bg)
                .bg(palette.peach)
                .add_modifier(Modifier::BOLD),
        ));
    }
    let full: String = pieces.iter().map(|(text, _)| text.as_str()).collect();
    let width = (full.chars().count() as u16).min(cap).max(1);
    let strip = Rect::new(frame.x + frame.width - width, frame.y, width, 1);
    buf.set_style(strip, Style::default().bg(palette.surface_dim));
    let mut x = strip.x;
    for (text, style) in pieces {
        let room = (strip.x + strip.width).saturating_sub(x);
        if room == 0 {
            break;
        }
        let text = truncate_end(&text, room as usize);
        let w = (text.chars().count() as u16).min(room);
        put_text(buf, x, strip.y, w, &text, style);
        x += w;
    }
    let mut hits = HollywoodRender {
        area: strip,
        ..HollywoodRender::default()
    };
    hits.rows.push((strip, Target::Title));
    hits
}

/// A role state's colour: live green, lost red, closed teal, anything
/// else dim — shared by the strip and the title row.
pub(crate) fn state_color(state: &str, palette: &Palette) -> Color {
    match state {
        "live" => palette.green,
        "lost" => palette.red,
        "closed" => palette.teal,
        _ => palette.overlay0,
    }
}

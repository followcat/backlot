//! The band under the frames: row dispatch and the single-line
//! headline/calm painters. Every cell names its own background — the
//! band floats over the panes.
//!
//! Waiting items live in `band_wait.rs` and `band_detail.rs`; the
//! lower-band rows (`live`, `tell hollywood`, `setup`) live in
//! `band_lower.rs`. The rows are drawn at rects the height ladder
//! chose in `draw`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::Hollywood;
use super::draw::HollywoodRender;
use super::rows::{HollywoodActivity, Row};
use super::sheet::Wait;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::truncate_end;

/// The space the band keeps at each side.
pub(crate) const MARGIN: u16 = 2;

/// Draw one band row across `rect`. `selected` lights the row and
/// its primary chip; `detail` draws the item's second line (which
/// `rect.height` already counts); `mobile` folds every row's chips
/// to `[k]` — the labels matter more than the chip words at these
/// widths.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_row(
    buf: &mut Buffer,
    rect: Rect,
    row: &Row<'_>,
    waits: &[Wait<'_>],
    hollywood: &Hollywood,
    local: &str,
    multi: bool,
    mobile: bool,
    suppress_highlights: bool,
    activity: HollywoodActivity,
    now_ms: u64,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    let elev = palette.elevation();
    let target = row.target(waits);
    let selected = !suppress_highlights && hollywood.selected.as_ref() == Some(&target);
    match row {
        Row::Headline(headline) => {
            let base = Style::default().bg(elev.surface);
            put_text(
                buf,
                rect.x + MARGIN,
                rect.y,
                1,
                "✦",
                base.fg(palette.accent),
            );
            let width = rect.width.saturating_sub(MARGIN * 2 + 2);
            put_text(
                buf,
                rect.x + MARGIN + 2,
                rect.y,
                width,
                &truncate_end(headline, width as usize),
                base.fg(palette.subtext0).add_modifier(Modifier::ITALIC),
            );
        }
        Row::Wait(i) => {
            let detail = rect.height > 1;
            super::band_wait::draw_wait(
                buf,
                Rect::new(rect.x, rect.y, rect.width, 1),
                rect.height,
                &waits[*i],
                waits,
                hollywood,
                local,
                multi,
                mobile,
                selected,
                palette,
                hits,
            );
            if detail {
                super::band_detail::draw_detail(
                    buf,
                    Rect::new(rect.x, rect.y + 1, rect.width, rect.height - 1),
                    &waits[*i],
                    hollywood,
                    selected,
                    palette,
                );
            }
        }
        Row::Calm { studios } => {
            let base = Style::default().bg(elev.surface);
            put_text(buf, rect.x + MARGIN, rect.y, 1, "✓", base.fg(palette.teal));
            let text = format!(" nothing waiting on you across {studios} studios");
            put_text(
                buf,
                rect.x + MARGIN + 1,
                rect.y,
                rect.width.saturating_sub(MARGIN + 1),
                &truncate_end(&text, rect.width as usize),
                base.fg(elev.muted_surface),
            );
        }
        Row::Talk(line) => {
            super::talk::draw_talk(buf, rect, line, selected, now_ms, palette);
        }
        Row::Fold { .. } => {}
        Row::Live(live) => super::band_lower::draw_live(buf, rect, live, selected, palette),
        Row::Input => {
            let handoff_active = waits.iter().any(|w| match w {
                Wait::Hollywood(hw) => super::handoff_ui::is_handoff_active(hw),
                _ => false,
            });
            super::band_lower::draw_input(
                buf,
                rect,
                hollywood,
                selected,
                suppress_highlights,
                activity,
                handoff_active,
                palette,
                hits,
            );
        }
        Row::Setup => super::band_lower::draw_setup(buf, rect, palette),
    }
    hits.rows.push((rect, target));
}

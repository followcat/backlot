//! The band under the title: row dispatch across WAITING and
//! HOLLYWOOD rows (`Wait`, `Headline`, `Talk`, `Live`, `Input`, `Setup`).
//! Every cell names its own background — the band floats over panes.

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

/// Extra display context for row dispatch (`WAITING` gutter, fallback
/// scroll counter, inline folded-turn count on unfocused talk rows).
#[derive(Clone, Copy, Default)]
pub(super) struct RowExtras {
    pub wait_focused: bool,
    pub wait_scroll_badge: Option<(usize, usize, usize)>,
    pub talk_earlier: usize,
}

/// Draw one band row across `rect` and record its hit rect.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_row(
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
    extras: RowExtras,
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
            let zone_focused = extras.wait_focused && !suppress_highlights;
            super::band_wait::draw_wait(
                buf,
                Rect::new(rect.x, rect.y, rect.width, 1),
                rect.height,
                &waits[*i],
                hollywood,
                local,
                multi,
                mobile,
                selected,
                zone_focused,
                extras.wait_scroll_badge,
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
                    zone_focused,
                    palette,
                );
            }
        }
        Row::Fold { .. } => {}
        Row::Talk(line) => {
            super::talk::draw_talk(
                buf,
                rect,
                line,
                selected,
                now_ms,
                extras.talk_earlier,
                palette,
                hits,
            );
        }
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

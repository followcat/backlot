//! Wait-row action chip strip painter and hit-map registration.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::draw::HollywoodRender;
use super::items;
use super::sheet::Wait;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::display_width;

/// Pixel/cell gap between adjacent action chips on a waiting row.
pub(crate) const CHIP_GAP: u16 = 1;

/// A waiting row's action chips (S5 §2), painted right-aligned from
/// `x` — the focused chip lit `accent` on the selected row — and
/// recorded as full-row-height click targets: `↵` maps to the row's
/// Enter, a letter to the row's own key.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_wait_chips(
    buf: &mut Buffer,
    y: u16,
    mut x: u16,
    row_height: u16,
    wait: &Wait<'_>,
    chips: &[items::Chip],
    compact: bool,
    selected: bool,
    focused_chip: Option<usize>,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    let start_x = x;
    let target = wait.target();
    let hit_h = row_height.max(1);
    let elev = palette.elevation();
    for (idx, chip) in chips.iter().enumerate() {
        let text = items::chip_text(chip, compact);
        let w = display_width(&text) as u16;
        let is_focused = selected && focused_chip == Some(idx);
        let style = if is_focused {
            if elev.is_terminal_16 {
                Style::default()
                    .fg(palette.text)
                    .add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else {
                Style::default()
                    .fg(crate::studio::glyphs::contrast_fg(palette))
                    .bg(palette.accent)
                    .add_modifier(Modifier::BOLD)
            }
        } else {
            let chip_bg = if elev.is_dark {
                palette.surface0
            } else {
                elev.surface
            };
            Style::default().fg(palette.subtext0).bg(chip_bg)
        };
        put_text(buf, x, y, w, &text, style);
        let key = if chip.key == "↵" {
            '\r'
        } else {
            chip.key.chars().next().unwrap_or('\r')
        };
        hits.chips
            .push((Rect::new(x, y, w, hit_h), target.clone(), key));
        x += w;
        if idx + 1 < chips.len() {
            x += CHIP_GAP;
        }
    }
    let total_w = x.saturating_sub(start_x);
    if total_w > 0 {
        hits.chips_zones
            .push((Rect::new(start_x, y, total_w, hit_h), target));
    }
}

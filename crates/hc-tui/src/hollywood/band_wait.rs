//! Waiting-item main row painter and segmented headline helper.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::Hollywood;
use super::band::MARGIN;
use super::draw::{CHIP_GAP, HollywoodRender};
use super::items::{chip_text, chips_of, default_chip_index, target_label, ultra_head};
use super::sheet::Wait;
use crate::chrome::palette::Palette;
use crate::chrome::render::{put_role_badged_text, put_text};
use crate::chrome::text::{display_width, truncate_end};

/// Draw a waiting row's main line: col-0 `▌` across the entire focused
/// WAITING zone, `▸` + action chips on the selected row, severity bar,
/// glyph + head text, and an optional `{top}–{bot}/{total}` scroll mark
/// when the title row has shed.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_wait(
    buf: &mut Buffer,
    rect: Rect,
    row_height: u16,
    wait: &Wait<'_>,
    hollywood: &Hollywood,
    local: &str,
    multi: bool,
    mobile: bool,
    selected: bool,
    zone_focused: bool,
    scroll_badge: Option<(usize, usize, usize)>,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    let elev = palette.elevation();
    let bg = if selected {
        elev.raised_row
    } else {
        elev.surface
    };
    buf.set_style(rect, Style::default().bg(bg));
    let stopped_server = matches!(wait, Wait::Stage(item) if !item.offline && item.machine.is_none())
        && hollywood.server_stopped_cmd.is_some();
    let colour = super::items::glyph_colour(wait, palette);
    let mut parts = if stopped_server {
        super::items::WaitParts {
            glyph: "◌",
            verb: hollywood.server_stopped_cmd.clone().unwrap_or_default(),
            gap: "",
            studio: String::new(),
            rest: String::new(),
        }
    } else {
        super::items::wait_parts(wait, local, multi)
    };
    if rect.width < 90 && parts.gap.len() > 1 {
        parts.gap = " ";
    }
    let head_plain = format!("{}{}{}{}", parts.verb, parts.gap, parts.studio, parts.rest);
    if zone_focused || selected {
        put_text(
            buf,
            rect.x,
            rect.y,
            1,
            "▌",
            Style::default().fg(palette.accent).bg(bg),
        );
    }
    if selected {
        put_text(
            buf,
            rect.x + 1,
            rect.y,
            1,
            "▸",
            Style::default().fg(palette.accent).bg(bg),
        );
    }
    let bar_x = rect.x + MARGIN;
    put_text(
        buf,
        bar_x,
        rect.y,
        1,
        "▎",
        Style::default().fg(colour).bg(bg),
    );
    let glyph_x = bar_x + 2;
    put_text(
        buf,
        glyph_x,
        rect.y,
        1,
        parts.glyph,
        Style::default()
            .fg(colour)
            .bg(bg)
            .add_modifier(Modifier::BOLD),
    );
    let chips = if stopped_server || !selected {
        Vec::new()
    } else {
        chips_of(wait)
    };
    let gaps = chips.len().saturating_sub(1) as u16 * CHIP_GAP;
    let full_w: u16 = chips
        .iter()
        .map(|c| display_width(&chip_text(c, false)) as u16)
        .sum::<u16>()
        + gaps;
    let compact_w: u16 = chips
        .iter()
        .map(|c| display_width(&chip_text(c, true)) as u16)
        .sum::<u16>()
        + gaps;
    let head_x = glyph_x + 2;
    let avail = rect.x + rect.width - MARGIN;
    let badge = scroll_badge.map(|(top, bot, total)| format!("{top}–{bot}/{total}"));
    let badge_reserve = badge.as_deref().map_or(0, |b| {
        display_width(b) as u16 + u16::from(!chips.is_empty()) + 1
    });
    let head_room_full = avail.saturating_sub(head_x + full_w + badge_reserve + 1);
    let compact = mobile || head_room_full < 20.min(display_width(&head_plain) as u16);
    let chips_w = if compact { compact_w } else { full_w };
    let head_room = avail.saturating_sub(head_x + chips_w + badge_reserve + 1);

    let drawn_w = if head_room < 12 && matches!(wait, Wait::Prompt(_)) {
        let ultra = ultra_head(wait, local, multi);
        let clipped = truncate_end(&ultra, head_room as usize);
        put_text(
            buf,
            head_x,
            rect.y,
            head_room,
            &clipped,
            Style::default().fg(palette.text).bg(bg),
        );
        display_width(&clipped) as u16
    } else {
        let segs = [
            (
                parts.verb.as_str(),
                Style::default()
                    .fg(colour)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ),
            (parts.gap, Style::default().bg(bg)),
            (
                parts.studio.as_str(),
                Style::default()
                    .fg(palette.text)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ),
            (
                parts.rest.as_str(),
                Style::default().fg(palette.subtext0).bg(bg),
            ),
        ];
        paint_head_segs(buf, head_x, rect.y, head_room, &segs, palette)
    };
    if let Some(pending) = hollywood.pending.as_deref()
        && pending.contains(target_label(wait))
    {
        let px = head_x + drawn_w.min(head_room);
        put_text(
            buf,
            px,
            rect.y,
            avail.saturating_sub(px),
            " …",
            Style::default().fg(elev.muted_surface).bg(bg),
        );
    }
    if let Some(mark) = badge.as_deref() {
        let mw = display_width(mark) as u16;
        let mx = avail.saturating_sub(chips_w + u16::from(chips_w > 0) + mw);
        if mx > head_x + drawn_w.min(head_room) {
            put_text(
                buf,
                mx,
                rect.y,
                mw,
                mark,
                Style::default().fg(elev.muted_surface).bg(bg),
            );
        }
    }
    let focused_chip = selected.then(|| {
        hollywood
            .chip_index
            .unwrap_or_else(|| default_chip_index(wait, &chips))
            .min(chips.len().saturating_sub(1))
    });
    super::draw::paint_wait_chips(
        buf,
        rect.y,
        avail - chips_w,
        row_height,
        wait,
        &chips,
        compact,
        selected,
        focused_chip,
        palette,
        hits,
    );
}

fn paint_head_segs(
    buf: &mut Buffer,
    mut x: u16,
    y: u16,
    room: u16,
    segs: &[(&str, Style)],
    palette: &Palette,
) -> u16 {
    let mut used = 0u16;
    for (seg, style) in segs {
        let rem = room.saturating_sub(used);
        if rem == 0 {
            break;
        }
        if seg.is_empty() {
            continue;
        }
        let clipped = truncate_end(seg, rem as usize);
        let w = (display_width(&clipped) as u16).min(rem);
        put_role_badged_text(buf, x, y, w, &clipped, *style, palette);
        x += w;
        used += w;
        if display_width(seg) > rem as usize {
            break;
        }
    }
    used
}

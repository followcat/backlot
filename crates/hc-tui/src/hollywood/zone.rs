//! Zone label crossbars and sprocket headers for the Call Sheet band:
//! - focused `╠══╡ ‹✦› H O L L Y W O O D ‹✦› ╞══ … ╞ <fold> ╞══╣`,
//! - unfocused spare-only `╟── HOLLYWOOD ── … ── <age/earlier> ──╢`,
//! - centered `╠═▄▄══▄▄═╡ ‹✦› S T U D I O S · N ‹✦› ╞═▄▄══▄▄═╣` sprocket
//!   header above expanded studio frames.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::Target;
use super::draw::HollywoodRender;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};

/// Render the centered 35mm sprocket crossbar above focused studio frames:
/// `╠═▄▄══▄▄═╡ ‹✦› S T U D I O S · N ‹✦› ╞═▄▄══▄▄═╣`.
pub(crate) fn draw_studios_perforation(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    studios: usize,
    focused: bool,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    if area.width == 0 {
        return;
    }
    let elev = palette.elevation();
    let bg = elev.surface;
    let brass = elev.gilt_style(bg);
    let bulb = Style::default().fg(elev.bulb_warm).bg(bg);
    let mark_style = if focused {
        Style::default()
            .fg(palette.accent)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(elev.muted_surface).bg(bg)
    };

    put_text(buf, area.x, y, 1, "╠", brass);
    if area.width > 1 {
        put_text(buf, area.x + area.width - 1, y, 1, "╣", brass);
    }
    if area.width < 4 {
        return;
    }

    let (flanks, text) = if area.width >= 120 {
        (true, format!(" S T U D I O S · {studios} "))
    } else if area.width >= 100 {
        (false, format!("✦ S T U D I O S · {studios}"))
    } else {
        (false, format!("✦ STUDIOS · {studios}"))
    };
    let core_w = display_width(&text) as u16 + if flanks { 6 } else { 0 };
    // Full plate: `╡ <core> ╞`
    let plate_w = 4 + core_w;
    if plate_w + 2 > area.width {
        let clipped = truncate_end(&format!("STUDIOS · {studios}"), area.width as usize - 2);
        let cw = display_width(&clipped) as u16;
        let lx = area.x + (area.width - cw) / 2;
        put_text(buf, lx, y, cw, &clipped, mark_style);
        hits.studios_label = Rect::new(lx, y, cw, 1);
        return;
    }
    let px = area.x + (area.width - plate_w) / 2;
    let px_end = px + plate_w;
    draw_sprocket_span(buf, area.x + 1, px, y, brass, bulb);
    draw_sprocket_span(buf, px_end, area.x + area.width - 1, y, brass, bulb);

    let mut cx = px;
    put_text(buf, cx, y, 2, "╡ ", brass);
    cx += 2;
    if flanks {
        put_text(buf, cx, y, 1, "‹", brass);
        put_text(buf, cx + 1, y, 1, "✦", mark_style);
        put_text(buf, cx + 2, y, 1, "›", brass);
        cx += 3;
        let tw = display_width(&text) as u16;
        put_text(buf, cx, y, tw, &text, mark_style);
        cx += tw;
        put_text(buf, cx, y, 1, "‹", brass);
        put_text(buf, cx + 1, y, 1, "✦", mark_style);
        put_text(buf, cx + 2, y, 1, "›", brass);
        cx += 3;
    } else {
        let tw = display_width(&text) as u16;
        put_text(buf, cx, y, tw, &text, mark_style);
        cx += tw;
    }
    put_text(buf, cx, y, 2, " ╞", brass);
    hits.studios_label = Rect::new(px, y, plate_w, 1);
}

fn draw_sprocket_span(buf: &mut Buffer, start: u16, end: u16, y: u16, brass: Style, bulb: Style) {
    let mut x = start;
    while x < end {
        let off = (x - start) % 4;
        if (off == 1 || off == 2) && x + 2 <= end {
            put_text(buf, x, y, 2, "▄▄", bulb);
            x += 2;
        } else {
            put_text(buf, x, y, 1, "═", brass);
            x += 1;
        }
    }
}

/// Fold metadata shown on the right end of focused `HOLLYWOOD` crossbar.
#[derive(Clone, Copy)]
pub(crate) struct FoldBadge {
    pub count: usize,
    pub newest: u64,
    pub selected: bool,
}

/// Render either focused `╠══╡ … HOLLYWOOD … ╞══ … ╞ <fold> ╞══╣` or
/// unfocused spare-only `╟── HOLLYWOOD ── … ── <right> ──╢`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_hollywood_rule(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    focused: bool,
    fold: Option<FoldBadge>,
    unfocused_right: Option<&str>,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    if area.width < 4 {
        return;
    }
    if !focused {
        draw_unfocused_hollywood_rule(buf, area, y, unfocused_right, palette);
        return;
    }
    let elev = palette.elevation();
    let bg = elev.surface;
    let brass = elev.gilt_style(bg);
    let mark_style = Style::default()
        .fg(palette.accent)
        .bg(bg)
        .add_modifier(Modifier::BOLD);
    let end = area.x + area.width;

    let mut x = area.x;
    if area.width >= 120 {
        put_text(buf, x, y, 4, "╠══╡", brass);
        x += 4;
        put_text(buf, x, y, 2, " ‹", brass);
        put_text(buf, x + 2, y, 1, "✦", mark_style);
        put_text(buf, x + 3, y, 2, "› ", brass);
        x += 5;
        put_text(buf, x, y, 18, "H O L L Y W O O D ", mark_style);
        x += 18;
        put_text(buf, x, y, 1, "‹", brass);
        put_text(buf, x + 1, y, 1, "✦", mark_style);
        put_text(buf, x + 2, y, 3, "› ╞", brass);
        x += 5;
    } else if area.width >= 100 {
        put_text(buf, x, y, 5, "╠══╡ ", brass);
        put_text(buf, x + 5, y, 20, "✦ H O L L Y W O O D ", mark_style);
        put_text(buf, x + 25, y, 1, "╞", brass);
        x += 26;
    } else {
        put_text(buf, x, y, 3, "╠═╡", brass);
        put_text(buf, x + 3, y, 11, "✦ HOLLYWOOD", mark_style);
        put_text(buf, x + 14, y, 1, "╞", brass);
        x += 15;
    }

    let cap = if area.width >= 100 {
        "╞══╣"
    } else {
        "╞═╣"
    };
    let cap_bare = if area.width >= 100 {
        "══╣"
    } else {
        "═╣"
    };
    let cap_w = display_width(cap) as u16;
    let bare_w = display_width(cap_bare) as u16;

    if let Some(fold) = fold {
        let pad = if area.width >= 100 { " " } else { "" };
        let full = if fold.selected {
            format!("{pad}▸ {} earlier · ↵ read{pad}", fold.count)
        } else {
            format!("{pad}{} earlier · ↵ read{pad}", fold.count)
        };
        let mid = if fold.selected {
            format!("{pad}▸ {} earlier · ↵{pad}", fold.count)
        } else {
            format!("{pad}{} earlier · ↵{pad}", fold.count)
        };
        let short = if fold.selected {
            format!("{pad}▸ {} earlier{pad}", fold.count)
        } else {
            format!("{pad}{} earlier{pad}", fold.count)
        };
        let budget = end.saturating_sub(x + cap_w + 1);
        let badge = [full, mid, short]
            .into_iter()
            .find(|s| display_width(s) as u16 <= budget)
            .unwrap_or_else(|| truncate_end(&format!("{}e", fold.count), budget as usize));
        let bw = display_width(&badge) as u16;
        if bw > 0 && x + 1 + bw + cap_w <= end {
            let right_clasp_x = end - cap_w;
            let pill_x = right_clasp_x - bw;
            let pill_bg = if fold.selected {
                elev.raised_row
            } else if elev.is_dark {
                palette.surface0
            } else {
                bg
            };
            let badge_style = if fold.selected {
                Style::default()
                    .fg(palette.text)
                    .bg(pill_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(elev.muted_surface).bg(pill_bg)
            };
            if pill_x > x + 1 {
                let fill = pill_x - 1 - x;
                put_text(buf, x, y, fill, &"═".repeat(fill as usize), brass);
            }
            put_text(buf, pill_x - 1, y, 1, "╡", brass);
            let pill_rect = Rect::new(pill_x, y, bw, 1);
            buf.set_style(pill_rect, badge_style);
            put_text(buf, pill_x, y, bw, &badge, badge_style);
            if fold.selected {
                let offset = display_width(pad) as u16;
                if pill_x + offset < end {
                    put_text(
                        buf,
                        pill_x + offset,
                        y,
                        1,
                        "▸",
                        badge_style.fg(palette.accent),
                    );
                }
            }
            put_text(buf, right_clasp_x, y, cap_w, cap, brass);
            hits.rows.push((pill_rect, Target::Talk(fold.newest)));
            return;
        }
    }

    if x + bare_w <= end {
        let fill = end - bare_w - x;
        if fill > 0 {
            put_text(buf, x, y, fill, &"═".repeat(fill as usize), brass);
        }
        put_text(buf, end - bare_w, y, bare_w, cap_bare, brass);
    }
}

fn draw_unfocused_hollywood_rule(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    unfocused_right: Option<&str>,
    palette: &Palette,
) {
    let elev = palette.elevation();
    let bg = elev.surface;
    let gilt = elev.gilt_style(bg);
    let label_style = Style::default().fg(palette.subtext0).bg(bg);
    let muted = Style::default().fg(elev.muted_surface).bg(bg);

    let (left_lead, right_tail) = if area.width >= 100 {
        ("╟── ", " ──╢")
    } else {
        ("╟─ ", " ─╢")
    };
    let mut x = area.x;
    let end = area.x + area.width;
    let lw = display_width(left_lead) as u16;
    put_text(buf, x, y, lw, left_lead, gilt);
    x += lw;
    put_text(buf, x, y, 9, "HOLLYWOOD", label_style);
    x += 9;
    if x < end {
        put_text(buf, x, y, 1, " ", gilt);
        x += 1;
    }

    let tw = display_width(right_tail) as u16;
    let right_end = end.saturating_sub(tw);
    let right_draw_x = if let Some(right) = unfocused_right.filter(|s| !s.is_empty()) {
        let rw = display_width(right) as u16;
        if x + rw + 2 <= right_end {
            let rx = right_end - rw;
            put_text(buf, rx - 1, y, 1, " ", gilt);
            put_text(buf, rx, y, rw, right, muted);
            put_text(buf, right_end, y, tw, right_tail, gilt);
            rx - 1
        } else {
            let bare_tail = right_tail.trim_start();
            let bw = display_width(bare_tail) as u16;
            put_text(buf, end - bw, y, bw, bare_tail, gilt);
            end - bw
        }
    } else {
        let bare_tail = right_tail.trim_start();
        let bw = display_width(bare_tail) as u16;
        put_text(buf, end - bw, y, bw, bare_tail, gilt);
        end - bw
    };

    if right_draw_x > x {
        let fill = right_draw_x - x;
        put_text(buf, x, y, fill, &"─".repeat(fill as usize), gilt);
    }
}

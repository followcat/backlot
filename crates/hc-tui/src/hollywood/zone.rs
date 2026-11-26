//! Zone label rules and optional hairlines for the Call Sheet band:
//! `── STUDIOS · <S> ──`, `── WAITING ON YOU · <N> ──`, and
//! `── HOLLYWOOD ──` (with optional fold target `k earlier · ↵ read`).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::Target;
use super::band::MARGIN;
use super::draw::HollywoodRender;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};

/// Render `── STUDIOS · <count> ────…──`.
pub(crate) fn draw_studios_rule(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    studios: usize,
    focused: bool,
    palette: &Palette,
) {
    let label = format!("STUDIOS · {studios}");
    draw_rule(
        buf,
        area,
        y,
        &[(&label, label_style(focused, palette))],
        palette,
    );
}

/// Render `── WAITING ON YOU · <total> ────…──`, optionally ending with
/// the scroll window `<top>–<bot>/<total> ──` when items overflow.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_waiting_rule(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    total: usize,
    user_urgent: bool,
    focused: bool,
    scroll_range: Option<(usize, usize)>,
    palette: &Palette,
) {
    let elev = palette.elevation();
    let count_fg = if user_urgent {
        palette.red
    } else {
        palette.peach
    };
    let count_str = format!(" · {total}");
    let left_segs = [
        ("WAITING ON YOU", label_style(focused, palette)),
        (
            count_str.as_str(),
            Style::default()
                .fg(count_fg)
                .bg(elev.surface)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    let right_badge = scroll_range.map(|(top, bot)| format!("{top}–{bot}/{total}"));
    if let Some(badge) = right_badge.as_deref() {
        draw_rule_with_right(
            buf,
            area,
            y,
            &left_segs,
            badge,
            Style::default().fg(elev.muted_surface).bg(elev.surface),
            None,
            palette,
        );
    } else {
        draw_rule(buf, area, y, &left_segs, palette);
    }
}

/// Fold metadata shown on the right end of `── HOLLYWOOD ──`.
#[derive(Clone, Copy)]
pub(crate) struct FoldBadge {
    pub count: usize,
    pub newest: u64,
    pub selected: bool,
}

/// Render `── HOLLYWOOD ────…──`, embedding ` <k> earlier · ↵ read `
/// at the right end (as `Target::Talk(newest)`) when turns fold.
pub(crate) fn draw_hollywood_rule(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    focused: bool,
    fold: Option<FoldBadge>,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    let elev = palette.elevation();
    let left_segs = [("HOLLYWOOD", label_style(focused, palette))];
    let Some(fold) = fold else {
        draw_rule(buf, area, y, &left_segs, palette);
        return;
    };
    let full = if fold.selected {
        format!("▸ {} earlier · ↵ read", fold.count)
    } else {
        format!("{} earlier · ↵ read", fold.count)
    };
    let short = if fold.selected {
        format!("▸ {} earlier", fold.count)
    } else {
        format!("{} earlier", fold.count)
    };
    let span = area.width.saturating_sub(MARGIN * 2);
    // Reserve space for `── HOLLYWOOD  ` + `  ──`.
    let right_budget = span.saturating_sub(18);
    let badge = if display_width(&full) as u16 <= right_budget {
        full
    } else {
        truncate_end(&short, right_budget as usize)
    };
    let badge_style = if fold.selected {
        Style::default()
            .fg(palette.text)
            .bg(elev.raised_row)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(elev.muted_surface).bg(elev.surface)
    };
    if let Some(hit) = draw_rule_with_right(
        buf,
        area,
        y,
        &left_segs,
        &badge,
        badge_style,
        fold.selected.then_some(palette.accent),
        palette,
    ) {
        hits.rows.push((hit, Target::Talk(fold.newest)));
    }
}

/// Render a plain `border_muted` hairline above Setup when `[hollywood]`
/// is unconfigured.
pub(crate) fn draw_setup_hairline(buf: &mut Buffer, area: Rect, y: u16, palette: &Palette) {
    let width = area.width.saturating_sub(MARGIN * 2);
    if width == 0 {
        return;
    }
    let elev = palette.elevation();
    put_text(
        buf,
        area.x + MARGIN,
        y,
        width,
        &"─".repeat(width as usize),
        Style::default().fg(elev.border_muted).bg(elev.surface),
    );
}

fn label_style(focused: bool, palette: &Palette) -> Style {
    let elev = palette.elevation();
    if focused {
        Style::default()
            .fg(palette.accent)
            .bg(elev.surface)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(elev.muted_surface).bg(elev.surface)
    }
}

fn draw_rule_prefix(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    left_segs: &[(&str, Style)],
    line_style: Style,
) -> Option<(u16, u16)> {
    let start = area.x + MARGIN;
    let end = (area.x + area.width).saturating_sub(MARGIN);
    if end <= start {
        return None;
    }
    let mut x = start;
    if x + 3 <= end {
        put_text(buf, x, y, 3, "── ", line_style);
        x += 3;
    }
    for (text, style) in left_segs {
        let room = end.saturating_sub(x);
        if room == 0 {
            break;
        }
        let clipped = truncate_end(text, room as usize);
        let w = display_width(&clipped) as u16;
        put_text(buf, x, y, w, &clipped, *style);
        x += w;
    }
    Some((x, end))
}

fn fill_rule_rest(buf: &mut Buffer, mut x: u16, end: u16, y: u16, line_style: Style) {
    if x < end {
        put_text(buf, x, y, 1, " ", line_style);
        x += 1;
    }
    if x < end {
        let fill = end - x;
        put_text(buf, x, y, fill, &"─".repeat(fill as usize), line_style);
    }
}

fn draw_rule(buf: &mut Buffer, area: Rect, y: u16, left_segs: &[(&str, Style)], palette: &Palette) {
    let elev = palette.elevation();
    let line_style = Style::default().fg(elev.border_muted).bg(elev.surface);
    let Some((x, end)) = draw_rule_prefix(buf, area, y, left_segs, line_style) else {
        return;
    };
    fill_rule_rest(buf, x, end, y, line_style);
}

#[allow(clippy::too_many_arguments)]
fn draw_rule_with_right(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    left_segs: &[(&str, Style)],
    badge: &str,
    badge_style: Style,
    caret_fg: Option<ratatui::style::Color>,
    palette: &Palette,
) -> Option<Rect> {
    let elev = palette.elevation();
    let line_style = Style::default().fg(elev.border_muted).bg(elev.surface);
    let (x, end) = draw_rule_prefix(buf, area, y, left_segs, line_style)?;
    let badge_w = display_width(badge) as u16;
    // Right cluster: ` <badge> ──` (padding + badge + space + 2 dashes).
    let right_cluster = badge_w + 4;
    if badge_w == 0 || x + right_cluster + 2 > end {
        fill_rule_rest(buf, x, end, y, line_style);
        return None;
    }
    let pill_x = end - right_cluster;
    fill_rule_rest(buf, x, pill_x, y, line_style);
    let pill_rect = Rect::new(pill_x, y, badge_w + 2, 1);
    buf.set_style(pill_rect, badge_style);
    put_text(buf, pill_x + 1, y, badge_w, badge, badge_style);
    if let Some(fg) = caret_fg
        && badge.starts_with('▸')
    {
        put_text(buf, pill_x + 1, y, 1, "▸", badge_style.fg(fg));
    }
    put_text(buf, end - 2, y, 2, "──", line_style);
    Some(pill_rect)
}

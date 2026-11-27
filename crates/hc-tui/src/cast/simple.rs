//! The cast's one-row painters: the `shells` group header and the
//! `others` toggle. Split out of `draw.rs` to keep it under the
//! file-size guideline.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;

/// The `shells` group header — a dim label line between the people
/// and their shells.
pub(crate) fn draw_shells_header(buffer: &mut Buffer, rect: Rect, palette: &Palette) {
    put_text(
        buffer,
        rect.x,
        rect.y,
        rect.width,
        " shells",
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
}

/// The `others` row: ` others <n> ▸` collapsed, `▾` expanded — the
/// in-place expansion of herdr's own agent rows.
pub(crate) fn draw_others(
    buffer: &mut Buffer,
    rect: Rect,
    count: usize,
    expanded: bool,
    palette: &Palette,
) {
    let arrow = if expanded { "▾" } else { "▸" };
    put_text(
        buffer,
        rect.x,
        rect.y,
        rect.width,
        &format!(" others {count} {arrow}"),
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
}

/// Draw the cast section's top rule (`area.y`): `──╡ ✦ CAST ╞────` in
/// `gilt` (or `━━╡ ▸ CAST ╞━━━━` in `accent` when Cast is focused) in
/// studio mode, or the plain hairline in `--plain` / `source = "herdr"`.
pub(crate) fn draw_top_rule(
    buffer: &mut Buffer,
    area: Rect,
    region_focused: bool,
    studio_chrome: bool,
    palette: &Palette,
) {
    let elev = palette.elevation();
    if !studio_chrome {
        put_text(
            buffer,
            area.x,
            area.y,
            area.width,
            &(if region_focused { "━" } else { "─" }).repeat(area.width as usize),
            Style::default().fg(if region_focused {
                palette.accent
            } else {
                elev.border_muted
            }),
        );
        return;
    }
    if area.width == 0 {
        return;
    }
    let (prefix, rule_ch, style) = if region_focused {
        (
            "━━╡ ▸ CAST ╞",
            "━",
            Style::default()
                .fg(palette.accent)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )
    } else {
        let mut s = Style::default().fg(elev.gilt);
        if elev.is_terminal_16 {
            s = s.add_modifier(ratatui::style::Modifier::DIM);
        }
        ("──╡ ✦ CAST ╞", "─", s)
    };
    let pw = crate::chrome::text::display_width(prefix) as u16;
    let w = area.width;
    if w <= pw {
        put_text(
            buffer,
            area.x,
            area.y,
            w,
            &crate::chrome::text::truncate_end(prefix, w as usize),
            style,
        );
        return;
    }
    put_text(buffer, area.x, area.y, pw, prefix, style);
    let rem = w - pw;
    put_text(
        buffer,
        area.x + pw,
        area.y,
        rem,
        &rule_ch.repeat(rem as usize),
        style,
    );
}

/// Draw the subtle 1-row closed-role divider separating open roles from
/// closed roles in the Cast sidebar.
pub(crate) fn draw_closed_divider(
    buffer: &mut Buffer,
    rect: Rect,
    count: usize,
    studio_chrome: bool,
    palette: &Palette,
) {
    if rect.is_empty() {
        return;
    }
    let elev = palette.elevation();
    if studio_chrome {
        let rule_style = Style::default().fg(elev.border_muted);
        let mut dot_style = Style::default().fg(elev.gilt);
        if elev.is_terminal_16 {
            dot_style = dot_style.add_modifier(ratatui::style::Modifier::DIM);
        }
        let text_style = Style::default().fg(elev.muted_surface);
        let core = format!(" closed ({count}) ");
        let cw = crate::chrome::text::display_width(&core);
        let w = rect.width as usize;
        if w <= cw + 4 {
            let trimmed = core.trim();
            put_text(
                buffer,
                rect.x,
                rect.y,
                rect.width,
                &crate::chrome::text::truncate_end(trimmed, w),
                text_style,
            );
            return;
        }
        let prefix_segs = [
            ("── ", rule_style),
            ("·", dot_style),
            (&core, text_style),
            ("· ", dot_style),
        ];
        let mut x = rect.x;
        let right = rect.right();
        for (seg, style) in prefix_segs {
            let sw = crate::chrome::text::display_width(seg) as u16;
            let room = right.saturating_sub(x);
            if room == 0 {
                break;
            }
            let take = sw.min(room);
            put_text(buffer, x, rect.y, take, seg, style);
            x = x.saturating_add(take);
        }
        let rem = right.saturating_sub(x);
        if rem > 0 {
            put_text(
                buffer,
                x,
                rect.y,
                rem,
                &"─".repeat(rem as usize),
                rule_style,
            );
        }
        return;
    }
    let label = format!(" closed ({count}) ");
    let lw = crate::chrome::text::display_width(&label);
    let w = rect.width as usize;
    if lw >= w {
        put_text(
            buffer,
            rect.x,
            rect.y,
            rect.width,
            label.trim(),
            Style::default().fg(elev.muted_surface),
        );
        return;
    }
    let left = (w - lw) / 2;
    let right = w - lw - left;
    let rule_style = Style::default().fg(elev.border_muted);
    let mut x = rect.x;
    if left > 0 {
        put_text(
            buffer,
            x,
            rect.y,
            left as u16,
            &"┄".repeat(left),
            rule_style,
        );
        x = x.saturating_add(left as u16);
    }
    put_text(
        buffer,
        x,
        rect.y,
        lw as u16,
        &label,
        Style::default().fg(elev.muted_surface),
    );
    x = x.saturating_add(lw as u16);
    if right > 0 {
        put_text(
            buffer,
            x,
            rect.y,
            right as u16,
            &"┄".repeat(right),
            rule_style,
        );
    }
}

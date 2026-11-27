//! The cast's one-row painters: the `shells` group header and the
//! `others` toggle. Split out of `draw.rs` to keep it under the
//! file-size guideline.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::CastView;
use crate::chrome::palette::Palette;
use crate::chrome::render::{ShellHitMap, put_right_text, put_text};
use crate::chrome::text::{display_width, truncate_end};
use crate::focus::CastTarget;
use crate::studio::StudioKey;

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

/// Draw the interactive 1-row collapsed closed-roles line in Cast:
/// `── · N closed · actor-a×6 actor-b×10 ▸`, keeping `N closed` and `▸` whole,
/// recording `hits.cast_closed`, and painting the focus/remembered cursor.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_closed_fold_row(
    buffer: &mut Buffer,
    rect: Rect,
    studio: &StudioKey,
    count: usize,
    tokens: &[String],
    view: &CastView<'_>,
    hits: &mut ShellHitMap,
    palette: &Palette,
) {
    hits.cast_closed = Some((rect, studio.clone()));
    draw_closed_fold(buffer, rect, count, tokens, palette);
    let is_cursor = view.region_focused
        && matches!(
            &view.cursor_target,
            Some(CastTarget::ClosedRoles { studio: s }) if s == studio
        );
    let is_remembered = !view.region_focused
        && matches!(
            &view.remembered_target,
            Some(CastTarget::ClosedRoles { studio: s }) if s == studio
        );
    if is_cursor {
        buffer.set_style(rect, Style::default().bg(palette.selection_bg));
        put_text(
            buffer,
            rect.x,
            rect.y,
            1,
            "▌",
            Style::default().fg(palette.accent).bg(palette.selection_bg),
        );
    } else if is_remembered {
        put_text(
            buffer,
            rect.x,
            rect.y,
            1,
            "·",
            Style::default().fg(palette.overlay0),
        );
    }
}

/// Draw `── · N closed · <tokens> ▸` in divider style (rule, gilt dots,
/// muted text) with `▸` at the right end and `N closed` / `▸` kept whole.
pub(crate) fn draw_closed_fold(
    buffer: &mut Buffer,
    rect: Rect,
    count: usize,
    tokens: &[String],
    palette: &Palette,
) {
    if rect.is_empty() {
        return;
    }
    let elev = palette.elevation();
    let rule_style = Style::default().fg(elev.border_muted);
    let mut dot_style = Style::default().fg(elev.gilt);
    if elev.is_terminal_16 {
        dot_style = dot_style.add_modifier(Modifier::DIM);
    }
    let text_style = Style::default().fg(elev.muted_surface);
    let count_text = format!("{count} closed");
    let core = format!(" {count_text} ");
    let prefix_w = 3 + 1 + display_width(&core) + 2;
    let w = rect.width as usize;

    if w >= prefix_w + 2 {
        let mut x = rect.x;
        for (seg, style) in [
            ("── ", rule_style),
            ("·", dot_style),
            (core.as_str(), text_style),
            ("· ", dot_style),
        ] {
            let sw = display_width(seg) as u16;
            put_text(buffer, x, rect.y, sw, seg, style);
            x = x.saturating_add(sw);
        }
        let token_room = w - prefix_w - 2;
        if token_room > 0 && !tokens.is_empty() {
            let shown = truncate_end(&tokens.join(" "), token_room);
            put_text(buffer, x, rect.y, token_room as u16, &shown, text_style);
        }
        put_right_text(buffer, rect, rect.y, "▸", text_style);
        return;
    }
    if w > 1 + display_width(&core) {
        let core_w = display_width(&core) as u16;
        put_text(buffer, rect.x, rect.y, 1, "·", dot_style);
        put_text(buffer, rect.x + 1, rect.y, core_w, &core, text_style);
        put_right_text(buffer, rect, rect.y, "▸", text_style);
        return;
    }
    let label_w = display_width(&count_text);
    if w >= label_w + 2 {
        put_text(
            buffer,
            rect.x,
            rect.y,
            label_w as u16,
            &count_text,
            text_style,
        );
        put_right_text(buffer, rect, rect.y, "▸", text_style);
    } else if w >= 2 {
        let avail = w - 2;
        put_text(
            buffer,
            rect.x,
            rect.y,
            avail as u16,
            &truncate_end(&count_text, avail),
            text_style,
        );
        put_right_text(buffer, rect, rect.y, "▸", text_style);
    } else {
        put_right_text(buffer, rect, rect.y, "▸", text_style);
    }
}

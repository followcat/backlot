//! Lower Call Sheet row painters: Hollywood's live progress row,
//! the `tell hollywood` compose field, and the no-agent setup row.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::draw::HollywoodRender;
use super::rows::{HollywoodActivity, Live, SETUP_COMMAND, SETUP_LEAD};
use super::{Hollywood, MARGIN};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};

/// Hollywood's live row under the newest `user ›` line.
pub(super) fn draw_live(
    buf: &mut Buffer,
    rect: Rect,
    live: &Live<'_>,
    selected: bool,
    zone_focused: bool,
    palette: &Palette,
) {
    let elev = palette.elevation();
    let bg = if selected {
        elev.raised_row
    } else {
        elev.surface
    };
    buf.set_style(rect, Style::default().bg(bg));
    let (text, glyph) = super::items::live_head(live);
    let (colour, bold) = match live {
        Live::Pending => (elev.muted_surface, false),
        Live::HandoffQueued { .. } | Live::BlockedQueue(_) => (palette.peach, false),
        Live::Working(_) => (palette.yellow, false),
        Live::Prompt(_) => (palette.red, true),
        Live::Idle => (elev.muted_surface, false),
    };
    let mut x = rect.x;
    if zone_focused || selected {
        put_text(
            buf,
            x,
            rect.y,
            1,
            "▌",
            Style::default().fg(palette.accent).bg(bg),
        );
    } else {
        put_text(buf, x, rect.y, 1, "║", elev.gilt_style(elev.surface));
    }
    if selected {
        put_text(
            buf,
            x + 1,
            rect.y,
            1,
            "▸",
            Style::default().fg(palette.accent).bg(bg),
        );
    }
    if rect.width >= 2 {
        put_text(
            buf,
            rect.x + rect.width - 1,
            rect.y,
            1,
            "║",
            elev.gilt_style(elev.surface),
        );
    }
    x += MARGIN;
    let glyph_style = if bold {
        Style::default()
            .fg(colour)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colour).bg(bg)
    };
    let glyph_w = display_width(glyph) as u16;
    put_text(buf, x, rect.y, glyph_w, glyph, glyph_style);
    let text_x = x + glyph_w + 1;
    let width = rect.width.saturating_sub(MARGIN * 2 + glyph_w + 1);
    put_text(
        buf,
        text_x,
        rect.y,
        width,
        &truncate_end(&text, width as usize),
        Style::default().fg(palette.text).bg(bg),
    );
}

/// The rail-joined `tell hollywood` compose crossbar:
/// `╟─┤ › tell hollywood… ├─╢` resting / `╠═╡ › … ╞═╣` while editing.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_input(
    buf: &mut Buffer,
    rect: Rect,
    hollywood: &Hollywood,
    selected: bool,
    suppress_highlights: bool,
    activity: HollywoodActivity,
    handoff_active: bool,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    let elev = palette.elevation();
    let field = Rect::new(
        rect.x + MARGIN,
        rect.y,
        rect.width.saturating_sub(MARGIN * 2),
        1,
    );
    if field.width < 6 {
        return;
    }
    let field_bg = elev.raised_field;
    buf.set_style(field, Style::default().bg(field_bg));
    let editing = hollywood.editing && !suppress_highlights;
    let gilt_frame = elev.gilt_style(elev.surface);
    let gilt_field = elev.gilt_style(field_bg);

    let (left_frame, left_clasp, right_clasp, right_frame, left_style, prompt_style) = if editing {
        let active = Style::default()
            .fg(palette.accent)
            .bg(elev.surface)
            .add_modifier(Modifier::BOLD);
        let active_clasp = Style::default()
            .fg(palette.accent)
            .bg(field_bg)
            .add_modifier(Modifier::BOLD);
        ("╠═", "╡", "╞", "═╣", active, active_clasp)
    } else if selected && !suppress_highlights {
        let sel_prompt = Style::default().fg(palette.accent).bg(field_bg);
        ("╟─", "┤", "├", "─╢", gilt_frame, sel_prompt)
    } else {
        (
            "╟─",
            "┤",
            "├",
            "─╢",
            gilt_frame,
            Style::default().fg(elev.muted_raised).bg(field_bg),
        )
    };
    let left_clasp_style = if editing { prompt_style } else { gilt_field };
    put_text(buf, rect.x, rect.y, MARGIN, left_frame, left_style);
    put_text(buf, field.x, field.y, 1, left_clasp, left_clasp_style);
    put_text(buf, field.x + 2, field.y, 1, "›", prompt_style);
    put_text(
        buf,
        field.x + field.width - 1,
        field.y,
        1,
        right_clasp,
        gilt_field,
    );
    put_text(
        buf,
        rect.x + rect.width - MARGIN,
        rect.y,
        MARGIN,
        right_frame,
        gilt_frame,
    );
    hits.input = field;
    let glyph = crate::studio::glyphs::Person::Working.glyph(hollywood.spin);
    let badge = match activity {
        HollywoodActivity::Working if field.width >= 46 => {
            Some((format!(" {glyph} thinking… "), palette.accent))
        }
        HollywoodActivity::Working if field.width >= 30 => {
            Some((format!(" {glyph} thinking "), palette.accent))
        }
        HollywoodActivity::Queued if field.width >= 46 => {
            Some((format!(" {glyph} queued… "), palette.peach))
        }
        HollywoodActivity::Queued if field.width >= 30 => {
            Some((format!(" {glyph} queued "), palette.peach))
        }
        _ => None,
    };
    let right_clasp_x = field.x + field.width - 1;
    let chip_x = field.x + 4;
    let text_x = super::compose_chip::draw_compose_chip(
        buf,
        chip_x,
        field.y,
        right_clasp_x,
        hollywood.reply_to.as_ref(),
        palette,
    );
    let badge_w = badge.as_ref().map_or(0, |(b, _)| display_width(b) as u16);
    let text = Rect::new(
        text_x,
        field.y,
        right_clasp_x.saturating_sub(text_x).saturating_sub(badge_w),
        1,
    );
    let placeholder = match hollywood.reply_to.as_ref() {
        Some((_, role)) => format!("reply to {role}…"),
        None if handoff_active => "tell hollywood  (draft kept across switch)".to_owned(),
        None => "tell hollywood…".to_owned(),
    };
    if editing {
        hits.cursor = crate::chrome::text_editor::render(
            buf,
            text,
            &hollywood.input,
            Style::default().fg(palette.text).bg(field_bg),
        );
        if hollywood.input.as_str().is_empty() && text.width > 0 {
            put_text(
                buf,
                text.x,
                text.y,
                text.width,
                &truncate_end(&placeholder, text.width as usize),
                Style::default()
                    .fg(elev.muted_raised)
                    .bg(field_bg)
                    .add_modifier(Modifier::ITALIC),
            );
        }
    } else if hollywood.input.as_str().is_empty() {
        put_text(
            buf,
            text.x,
            text.y,
            text.width,
            &truncate_end(&placeholder, text.width as usize),
            Style::default()
                .fg(elev.muted_raised)
                .bg(field_bg)
                .add_modifier(Modifier::ITALIC),
        );
    } else {
        put_text(
            buf,
            text.x,
            text.y,
            text.width,
            &truncate_end(hollywood.input.as_str(), text.width as usize),
            Style::default()
                .fg(if suppress_highlights {
                    elev.muted_raised
                } else {
                    palette.text
                })
                .bg(field_bg),
        );
    }
    if let Some((badge, fg)) = badge {
        put_text(
            buf,
            field.x + field.width - 1 - badge_w,
            field.y,
            badge_w,
            &badge,
            Style::default().fg(fg).bg(field_bg),
        );
    }
}

pub(super) fn draw_setup(buf: &mut Buffer, rect: Rect, active_rail: bool, palette: &Palette) {
    let elev = palette.elevation();
    let base = Style::default().bg(elev.surface);
    if active_rail {
        put_text(buf, rect.x, rect.y, 1, "▌", base.fg(palette.accent));
    } else {
        put_text(buf, rect.x, rect.y, 1, "║", elev.gilt_style(elev.surface));
    }
    if rect.width >= 2 {
        put_text(
            buf,
            rect.x + rect.width - 1,
            rect.y,
            1,
            "║",
            elev.gilt_style(elev.surface),
        );
    }
    let x = rect.x + MARGIN;
    // R3-M13: the lead shortens before the button ever clips — the
    // full sentence while both fit, the short lead under ~84 cols,
    // the bare button at the narrowest.
    let room = rect.width.saturating_sub(MARGIN * 2);
    let cmd_w = display_width(SETUP_COMMAND);
    let lead = if room as usize >= display_width(SETUP_LEAD) + 1 + cmd_w {
        format!("{SETUP_LEAD} ")
    } else if room as usize >= display_width("✦ watch every studio:") + 1 + cmd_w {
        "✦ watch every studio: ".to_string()
    } else {
        String::new()
    };
    let lead_w = (display_width(&lead) as u16).min(room);
    if lead_w > 0 {
        put_text(buf, x, rect.y, lead_w, &lead, base.fg(elev.muted_surface));
    }
    let rest = room.saturating_sub(lead_w);
    if rest > 0 {
        put_text(
            buf,
            x + lead_w,
            rect.y,
            rest,
            &truncate_end(SETUP_COMMAND, rest as usize),
            base.fg(palette.subtext0),
        );
    }
}

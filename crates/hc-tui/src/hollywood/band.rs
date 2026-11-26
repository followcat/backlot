//! The band under the frames: the headline, one row per waiting
//! item — each with its action chips and, when kept, its detail
//! line — the calm row, the `tell hollywood` input line and the
//! no-agent setup line. Every cell names its own
//! background — the band floats over the panes.
//!
//! The rows are drawn at rects the height ladder chose in `draw`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::draw::{CHIP_GAP, HollywoodRender};
use super::items::{chip_text, chips_of, default_chip_index, live_head, target_label, ultra_head};
use super::rows::{HollywoodActivity, Live, Row, SETUP_COMMAND, SETUP_LEAD};
use super::sheet::Wait;
use super::{Confirm, Hollywood};
use crate::chrome::palette::Palette;
use crate::chrome::render::{put_role_badged_text, put_text};
use crate::chrome::text::{display_width, truncate_end};

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
            draw_wait(
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
                draw_detail(
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
        Row::Live(live) => draw_live(buf, rect, live, selected, palette),
        Row::Input => {
            let handoff_active = waits.iter().any(|w| match w {
                Wait::Hollywood(hw) => super::handoff_ui::is_handoff_active(hw),
                _ => false,
            });
            draw_input(
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
        Row::Setup => draw_setup(buf, rect, palette),
    }
    hits.rows.push((rect, target));
}

/// Draw a waiting row's main line: `▌▸` when selected, the glyph,
/// the head text, then right-aligned chips — the primary one lit
/// `accent` only on the selected row.
#[allow(clippy::too_many_arguments)]
fn draw_wait(
    buf: &mut Buffer,
    rect: Rect,
    row_height: u16,
    wait: &Wait<'_>,
    waits: &[Wait<'_>],
    hollywood: &Hollywood,
    local: &str,
    multi: bool,
    mobile: bool,
    selected: bool,
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
    let extra = super::rows::group_extra(waits, wait);
    if extra > 0 {
        parts.rest.push_str(&format!(" · +{extra}"));
    }
    if rect.width < 90 && parts.gap.len() > 1 {
        parts.gap = " ";
    }
    let head_plain = format!("{}{}{}{}", parts.verb, parts.gap, parts.studio, parts.rest);
    if selected {
        put_text(
            buf,
            rect.x,
            rect.y,
            1,
            "▌",
            Style::default().fg(palette.accent).bg(bg),
        );
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
    let head_room_full = avail.saturating_sub(head_x + full_w + 1);
    let compact = mobile || head_room_full < 20.min(display_width(&head_plain) as u16);
    let chips_w = if compact { compact_w } else { full_w };
    let head_room = avail.saturating_sub(head_x + chips_w + 1);

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

/// A waiting row's second line: indented directly under the head verb,
/// keeping the col-2 severity bar.
fn draw_detail(
    buf: &mut Buffer,
    rect: Rect,
    wait: &Wait<'_>,
    hollywood: &Hollywood,
    selected: bool,
    palette: &Palette,
) {
    let elev = palette.elevation();
    let bg = if selected {
        elev.raised_row
    } else {
        elev.surface
    };
    buf.set_style(rect, Style::default().bg(bg));
    if selected {
        put_text(
            buf,
            rect.x,
            rect.y,
            1,
            "▌",
            Style::default().fg(palette.accent).bg(bg),
        );
    }
    let colour = super::items::glyph_colour(wait, palette);
    put_text(
        buf,
        rect.x + MARGIN,
        rect.y,
        1,
        "▎",
        Style::default().fg(colour).bg(bg),
    );
    let x = rect.x + MARGIN + 4;
    let width = rect.width.saturating_sub(MARGIN * 2 + 4);
    if let Some(Confirm::Finish(key)) = hollywood.confirm.as_ref()
        && wait.target() == super::Target::Ready(key.clone())
    {
        let pending =
            matches!(wait, Wait::Ready(item) if crate::panel::data::report_pending(item.studio));
        let text = if pending {
            "finish studio (report pending)?  y yes   n no"
        } else {
            "finish studio?  y yes   n no"
        };
        put_text(
            buf,
            x,
            rect.y,
            width,
            &truncate_end(text, width as usize),
            Style::default()
                .fg(palette.peach)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        );
        return;
    }
    if let Wait::Decision(item) = wait {
        let lines = super::decision::detail_lines(item);
        for (idx, line) in lines.into_iter().take(rect.height as usize).enumerate() {
            let y = rect.y + idx as u16;
            if idx > 0 {
                if selected {
                    put_text(
                        buf,
                        rect.x,
                        y,
                        1,
                        "▌",
                        Style::default().fg(palette.accent).bg(bg),
                    );
                }
                put_text(
                    buf,
                    rect.x + MARGIN,
                    y,
                    1,
                    "▎",
                    Style::default().fg(colour).bg(bg),
                );
            }
            put_text(
                buf,
                x,
                y,
                width,
                &truncate_end(&line, width as usize),
                Style::default().fg(palette.subtext0).bg(bg),
            );
        }
        return;
    }
    if let Some(text) = super::items::detail_text(wait) {
        put_text(
            buf,
            x,
            rect.y,
            width,
            &truncate_end(&text, width as usize),
            Style::default().fg(palette.subtext0).bg(bg),
        );
        return;
    }
    let (text, colour) = match wait {
        Wait::Prompt(item) => (
            item.role
                .attention_detail
                .as_deref()
                .unwrap_or("unbound prompt — peek screen")
                .to_string(),
            palette.subtext0,
        ),
        Wait::Card(card) => (format!("why: {}", card.why), palette.subtext0),
        Wait::Outward(item) => {
            let commits = if item.card.commits_total > 0 {
                format!(
                    " · {} commit{}",
                    item.card.commits_total,
                    if item.card.commits_total == 1 {
                        ""
                    } else {
                        "s"
                    }
                )
            } else {
                String::new()
            };
            let remote = if item.card.remote_url.is_empty() {
                String::new()
            } else {
                format!(" · {}", item.card.remote_url)
            };
            (
                format!(
                    "{}{} · why: {}{}",
                    item.card.rule, commits, item.card.why, remote
                ),
                palette.subtext0,
            )
        }
        Wait::Ready(item) if crate::panel::data::report_pending(item.studio) => {
            ("report pending".to_string(), elev.muted_surface)
        }
        _ => return,
    };
    put_text(
        buf,
        x,
        rect.y,
        width,
        &truncate_end(&text, width as usize),
        Style::default().fg(colour).bg(bg),
    );
}

/// Hollywood's live row under the newest `user ›` line.
fn draw_live(buf: &mut Buffer, rect: Rect, live: &Live<'_>, selected: bool, palette: &Palette) {
    let elev = palette.elevation();
    let bg = if selected {
        elev.raised_row
    } else {
        elev.surface
    };
    buf.set_style(rect, Style::default().bg(bg));
    let (text, glyph) = live_head(live);
    let (colour, bold) = match live {
        Live::Pending => (elev.muted_surface, false),
        Live::HandoffQueued { .. } | Live::BlockedQueue(_) => (palette.peach, false),
        Live::Working(_) => (palette.yellow, false),
        Live::Prompt(_) => (palette.red, true),
        Live::Idle => (elev.muted_surface, false),
    };
    let mut x = rect.x;
    if selected {
        put_text(
            buf,
            x,
            rect.y,
            1,
            "▌",
            Style::default().fg(palette.accent).bg(bg),
        );
        put_text(
            buf,
            x + 1,
            rect.y,
            1,
            "▸",
            Style::default().fg(palette.accent).bg(bg),
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
    let width = rect.width.saturating_sub(MARGIN + glyph_w + 1);
    put_text(
        buf,
        text_x,
        rect.y,
        width,
        &truncate_end(&text, width as usize),
        Style::default().fg(palette.text).bg(bg),
    );
}

/// The `tell hollywood` line on `elevation.raised_field`.
#[allow(clippy::too_many_arguments)]
fn draw_input(
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
    let (bar_sym, bar_fg, prompt_style) = if editing {
        (
            "▌",
            palette.accent,
            Style::default()
                .fg(palette.accent)
                .bg(field_bg)
                .add_modifier(Modifier::BOLD),
        )
    } else if selected && !suppress_highlights {
        (
            "▎",
            palette.accent,
            Style::default().fg(palette.accent).bg(field_bg),
        )
    } else {
        (
            "▎",
            elev.border_muted,
            Style::default().fg(elev.muted_raised).bg(field_bg),
        )
    };
    put_text(
        buf,
        field.x,
        field.y,
        1,
        bar_sym,
        Style::default().fg(bar_fg).bg(field_bg),
    );
    put_text(buf, field.x + 2, field.y, 1, "›", prompt_style);
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
    let badge_w = badge.as_ref().map_or(0, |(b, _)| display_width(b) as u16);
    let text = Rect::new(
        field.x + 4,
        field.y,
        (field.width - 4).saturating_sub(badge_w),
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
            field.x + field.width - badge_w,
            field.y,
            badge_w,
            &badge,
            Style::default().fg(fg).bg(field_bg),
        );
    }
}

fn draw_setup(buf: &mut Buffer, rect: Rect, palette: &Palette) {
    let elev = palette.elevation();
    let x = rect.x + MARGIN;
    let base = Style::default().bg(elev.surface);
    // R3-M13: the lead shortens before the button ever clips — the
    // full sentence while both fit, the short lead under ~84 cols,
    // the bare button at the narrowest.
    let room = rect.width.saturating_sub(MARGIN);
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

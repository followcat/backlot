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

use backlot_model::summary::Exchange;

use super::draw::{CHIP_GAP, HollywoodRender};
use super::items::{
    chip_text, chips_of, default_chip_index, live_head, target_label, ultra_head, wait_head,
};
use super::rows::{HollywoodActivity, Live, Row, SETUP_COMMAND, SETUP_LEAD};
use super::sheet::Wait;
use super::{Confirm, Hollywood};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
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
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    let target = row.target(waits);
    let selected = !suppress_highlights && hollywood.selected.as_ref() == Some(&target);
    match row {
        Row::Headline(headline) => {
            let base = Style::default().bg(palette.surface_dim);
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
                    Rect::new(rect.x, rect.y + 1, rect.width, 1),
                    &waits[*i],
                    hollywood,
                    selected,
                    palette,
                );
            }
        }
        Row::Calm { studios } => {
            let base = Style::default().bg(palette.surface_dim);
            put_text(buf, rect.x + MARGIN, rect.y, 1, "✓", base.fg(palette.teal));
            let text = format!(" nothing waiting on you across {studios} studios");
            put_text(
                buf,
                rect.x + MARGIN + 1,
                rect.y,
                rect.width.saturating_sub(MARGIN + 1),
                &truncate_end(&text, rect.width as usize),
                base.fg(palette.overlay1),
            );
        }
        Row::Talk(line) => draw_talk(buf, rect, line, selected, palette, hits),
        Row::Live(live) => draw_live(buf, rect, live, selected, palette),
        Row::Input => draw_input(
            buf,
            rect,
            hollywood,
            selected,
            suppress_highlights,
            activity,
            palette,
            hits,
        ),
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
    hollywood: &Hollywood,
    local: &str,
    multi: bool,
    mobile: bool,
    selected: bool,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    let bg = if selected {
        palette.selection_bg
    } else {
        palette.surface_dim
    };
    buf.set_style(rect, Style::default().bg(bg));
    let (head, glyph) = wait_head(wait, local, multi);
    let glyph_colour = match wait {
        Wait::Prompt(_) => palette.red,
        Wait::Role(item) if item.asks => palette.peach,
        Wait::Role(_) => palette.red,
        Wait::Hollywood(hollywood) if hollywood.attention_user => palette.red,
        Wait::Hollywood(_) => palette.peach,
        Wait::NoRequirement(_) => palette.accent,
        Wait::Stage(item) if item.offline => palette.red,
        Wait::Stage(_) => palette.peach,
        Wait::Card(_) | Wait::Outward(_) => palette.accent,
        Wait::Ready(_) => palette.teal,
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
    put_text(
        buf,
        x,
        rect.y,
        1,
        glyph,
        Style::default()
            .fg(glyph_colour)
            .bg(bg)
            .add_modifier(Modifier::BOLD),
    );
    let chips = chips_of(wait);
    let gaps = chips.len().saturating_sub(1) as u16 * CHIP_GAP;
    // Compact chips at the narrow cut, and whenever the full labels
    // cannot leave the head its twenty cells.
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
    let head_x = x + 2;
    let avail = rect.x + rect.width - MARGIN;
    let head_room_full = avail.saturating_sub(head_x + full_w + 1);
    let compact = mobile || head_room_full < 20.min(display_width(&head) as u16);
    let chips_w = if compact { compact_w } else { full_w };
    let head_room = avail.saturating_sub(head_x + chips_w + 1);
    // Ultra-compact at the narrowest: `× #7 dir` — the kind word
    // drops out of the head.
    let head = if head_room < 12 && matches!(wait, Wait::Prompt(_)) {
        ultra_head(wait, local, multi)
    } else {
        head
    };
    let head_room = avail.saturating_sub(head_x + chips_w + 1);
    put_text(
        buf,
        head_x,
        rect.y,
        head_room,
        &truncate_end(&head, head_room as usize),
        Style::default().fg(palette.text).bg(bg),
    );
    // Pending mark on the row while its action runs.
    if let Some(pending) = hollywood.pending.as_deref()
        && pending.contains(target_label(wait))
    {
        put_text(
            buf,
            head_x + head_room.min(display_width(&head) as u16),
            rect.y,
            avail.saturating_sub(head_x + head_room.min(display_width(&head) as u16)),
            " …",
            Style::default().fg(palette.overlay0).bg(bg),
        );
    }
    let focused_chip = selected.then(|| {
        hollywood
            .chip_index
            .unwrap_or_else(|| default_chip_index(wait, &chips))
            .min(chips.len().saturating_sub(1))
    });
    // S5: the chips paint and record their click targets together.
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

/// A waiting row's second line: the prompt's detail or the card's
/// `why:` — the ready confirm's own line while it asks.
fn draw_detail(
    buf: &mut Buffer,
    rect: Rect,
    wait: &Wait<'_>,
    hollywood: &Hollywood,
    selected: bool,
    palette: &Palette,
) {
    let bg = if selected {
        palette.selection_bg
    } else {
        palette.surface_dim
    };
    buf.set_style(rect, Style::default().bg(bg));
    let x = rect.x + MARGIN + 2;
    let width = rect.width.saturating_sub(MARGIN * 2 + 2);
    // The finish confirm reuses the line while it asks `y/n` — an
    // early finish (the wrap report still owed) warns inside the ask
    // (R3-3). It draws on the wait the confirm targets, wherever the
    // selection sits — a frame's `f` arms the same row.
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
        // R3-3: the wrap report has not landed — a dim line under the
        // READY row says so, like the dock's `· report pending`.
        Wait::Ready(item) if crate::panel::data::report_pending(item.studio) => {
            ("report pending".to_string(), palette.overlay0)
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

/// One exchange line: `✦ <text>` for Hollywood's half (its reported
/// headline, the accent the headline row already carries), the
/// sender's `who ›` for every other line — dimmer, the dialog's
/// other side.
fn draw_talk(
    buf: &mut Buffer,
    rect: Rect,
    line: &Exchange,
    selected: bool,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    let _ = hits;
    let base = if selected {
        Style::default().bg(palette.active_row_bg)
    } else {
        Style::default().bg(palette.surface_dim)
    };
    buf.set_style(rect, base);
    let (marker, who_style, text_style) = if line.who == "hollywood" {
        (
            "✦".to_string(),
            base.fg(palette.accent),
            base.fg(palette.text),
        )
    } else {
        (
            format!("{} ›", line.who),
            base.fg(palette.subtext0),
            base.fg(palette.subtext0),
        )
    };
    let x = rect.x + MARGIN;
    let left = marker.chars().count() as u16;
    put_text(buf, x, rect.y, left, &marker, who_style);
    let width = rect.width.saturating_sub(MARGIN + left + 1);
    put_text(
        buf,
        x + left + 1,
        rect.y,
        width,
        &truncate_end(&line.text, width as usize),
        text_style,
    );
}

/// Hollywood's live row under the newest `user ›` line: `… sending to
/// hollywood`, `⠶ hollywood: <doing>` or `⚠ hollywood asks: <prompt>` —
/// one line, cut to the width, on the band's own background.
fn draw_live(buf: &mut Buffer, rect: Rect, live: &Live<'_>, selected: bool, palette: &Palette) {
    let bg = if selected {
        palette.active_row_bg
    } else {
        palette.surface_dim
    };
    buf.set_style(rect, Style::default().bg(bg));
    let (text, glyph) = live_head(live);
    let (colour, bold) = match live {
        Live::Pending => (palette.overlay0, false),
        Live::Working(_) => (palette.yellow, false),
        Live::Prompt(_) => (palette.red, true),
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

/// The `tell hollywood` line on `active_row_bg`: `›` in accent, then
/// the placeholder or the editor's text.
#[allow(clippy::too_many_arguments)]
fn draw_input(
    buf: &mut Buffer,
    rect: Rect,
    hollywood: &Hollywood,
    selected: bool,
    suppress_highlights: bool,
    activity: HollywoodActivity,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    let field = Rect::new(
        rect.x + MARGIN,
        rect.y,
        rect.width.saturating_sub(MARGIN * 2),
        1,
    );
    if field.width < 4 {
        return;
    }
    let field_bg = if suppress_highlights {
        palette.surface_dim
    } else {
        palette.active_row_bg
    };
    buf.set_style(field, Style::default().bg(field_bg));
    if selected {
        put_text(
            buf,
            rect.x,
            rect.y,
            1,
            "▌",
            Style::default().fg(palette.accent).bg(palette.surface_dim),
        );
        put_text(
            buf,
            rect.x + 1,
            rect.y,
            1,
            "▸",
            Style::default().fg(palette.accent).bg(palette.surface_dim),
        );
    }
    let prompt_style = if suppress_highlights {
        Style::default().fg(palette.overlay0).bg(field_bg)
    } else {
        Style::default()
            .fg(palette.accent)
            .bg(field_bg)
            .add_modifier(Modifier::BOLD)
    };
    put_text(buf, field.x + 1, field.y, 1, "›", prompt_style);
    hits.input = field;
    let badge = match activity {
        HollywoodActivity::Working if field.width >= 24 => Some((
            format!(
                " {} thinking… ",
                crate::studio::glyphs::Person::Working.glyph(hollywood.spin)
            ),
            palette.accent,
        )),
        HollywoodActivity::Queued if field.width >= 24 => Some((
            format!(
                " {} queued… ",
                crate::studio::glyphs::Person::Working.glyph(hollywood.spin)
            ),
            palette.peach,
        )),
        _ => None,
    };
    let badge_w = badge.as_ref().map_or(0, |(b, _)| display_width(b) as u16);
    let text = Rect::new(
        field.x + 3,
        field.y,
        (field.width - 3).saturating_sub(badge_w),
        1,
    );
    if hollywood.editing && !suppress_highlights {
        hits.cursor = crate::chrome::text_editor::render(
            buf,
            text,
            &hollywood.input,
            Style::default().fg(palette.text).bg(field_bg),
        );
    } else if hollywood.input.as_str().is_empty() {
        put_text(
            buf,
            text.x,
            text.y,
            text.width,
            &truncate_end("tell hollywood what you need…", text.width as usize),
            Style::default()
                .fg(palette.overlay0)
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
                    palette.overlay0
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

/// `✦ an agent can watch every studio and tell you what needs you:`
/// then `⏎ hollywood setup` — the lead in `overlay0`, the
/// actionable command in `subtext0`; Enter/click opens the in-TUI
/// dialog (U7, R22).
fn draw_setup(buf: &mut Buffer, rect: Rect, palette: &Palette) {
    let x = rect.x + MARGIN;
    let base = Style::default().bg(palette.surface_dim);
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
        put_text(buf, x, rect.y, lead_w, &lead, base.fg(palette.overlay0));
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

//! The band under the frames: the headline, one row per waiting
//! item — each with its action chips and, when kept, its detail
//! line — the calm row, the `tell hollywood` input line, the
//! no-agent setup line and the key hints. Every cell names its own
//! background — the band floats over the panes.
//!
//! The rows are drawn at rects the height ladder chose in `draw`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::draw::HollywoodRender;
use super::items::{chip_text, chips_of, target_label, ultra_head, wait_head};
use super::rows::{Row, SETUP_COMMAND, SETUP_LEAD};
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
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    let target = row.target(waits);
    let selected = hollywood.selected.as_ref() == Some(&target);
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
        Row::Calm(studios) => {
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
        Row::Input => draw_input(buf, rect, hollywood, selected, palette, hits),
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
        Wait::Stage(item) if item.offline => palette.red,
        Wait::Stage(_) => palette.peach,
        Wait::Card(_) => palette.accent,
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
    // Compact chips at the narrow cut, and whenever the full labels
    // cannot leave the head its twenty cells.
    let full_w: u16 = chips
        .iter()
        .map(|c| display_width(&chip_text(c, false)) as u16)
        .sum();
    let compact_w: u16 = chips
        .iter()
        .map(|c| display_width(&chip_text(c, true)) as u16)
        .sum();
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
    let mut cx = avail - chips_w;
    for chip in &chips {
        let text = chip_text(chip, compact);
        let w = display_width(&text) as u16;
        let style = if selected && chip.primary {
            Style::default()
                .fg(crate::studio::glyphs::contrast_fg(palette))
                .bg(palette.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.subtext0).bg(palette.surface0)
        };
        put_text(buf, cx, rect.y, w, &text, style);
        // S5: the chip is its own click target — `↵` means the row's
        // Enter, a letter the row's own key.
        let key = if chip.key == "↵" {
            '\r'
        } else {
            chip.key.chars().next().unwrap_or('\r')
        };
        hits.chips
            .push((Rect::new(cx, rect.y, w, 1), wait.target(), key));
        cx += w;
    }
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
    // The finish confirm reuses the line while it asks `y/n`.
    if selected
        && let Some(confirm) = hollywood.confirm.as_ref()
        && matches!(confirm, Confirm::Finish(_))
    {
        put_text(
            buf,
            x,
            rect.y,
            width,
            &truncate_end("finish studio?  y yes   n no", width as usize),
            Style::default()
                .fg(palette.peach)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
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

/// The `tell hollywood` line on `active_row_bg`: `›` in accent, then
/// the placeholder or the editor's text.
fn draw_input(
    buf: &mut Buffer,
    rect: Rect,
    hollywood: &Hollywood,
    selected: bool,
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
    buf.set_style(field, Style::default().bg(palette.active_row_bg));
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
    put_text(
        buf,
        field.x + 1,
        field.y,
        1,
        "›",
        Style::default()
            .fg(palette.accent)
            .bg(palette.active_row_bg)
            .add_modifier(Modifier::BOLD),
    );
    hits.input = field;
    let text = Rect::new(field.x + 3, field.y, field.width - 3, 1);
    if hollywood.editing {
        hits.cursor = crate::chrome::text_editor::render(
            buf,
            text,
            &hollywood.input,
            Style::default().fg(palette.text).bg(palette.active_row_bg),
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
                .bg(palette.active_row_bg)
                .add_modifier(Modifier::ITALIC),
        );
    } else {
        put_text(
            buf,
            text.x,
            text.y,
            text.width,
            &truncate_end(hollywood.input.as_str(), text.width as usize),
            Style::default().fg(palette.text).bg(palette.active_row_bg),
        );
    }
}

/// `✦ an agent can watch every studio and tell you what needs you:`
/// then `backlot hollywood setup …` — the lead in `overlay0`, the
/// command in `subtext0` (U7).
fn draw_setup(buf: &mut Buffer, rect: Rect, palette: &Palette) {
    let x = rect.x + MARGIN;
    let base = Style::default().bg(palette.surface_dim);
    let lead = format!("{SETUP_LEAD} ");
    let room = rect.width.saturating_sub(MARGIN);
    let lead_w = (lead.chars().count() as u16).min(room);
    put_text(
        buf,
        x,
        rect.y,
        lead_w,
        &truncate_end(&lead, lead_w as usize),
        base.fg(palette.overlay0),
    );
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

/// The adaptive hint row's tiers — the longest that fits wins.
pub(crate) const HINT_TIERS: [&str; 4] = [
    "↑↓ move   ⏎ open   y/d answer   a/x card   f finish   r start   p peek   i tell   m menu   ? help   esc close",
    "↑↓ move   ⏎ open   a/x card   f finish   i tell   esc close",
    "↑↓ ⏎ y/d a/x f i esc",
    "↑↓ ⏎ esc",
];

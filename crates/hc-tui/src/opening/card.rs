//! The film-strip title card and its fallback ladder.
//!
//! Full size: a 47-column card of 13 rows with three actions, two
//! sprocket rows and the footer. Step by step for smaller panes:
//! drop the sprockets, replace the art title with `B A C K L O T`,
//! then drop the frame (the plain layout in `opening/mod.rs`).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::{Action, OPENING_FRAMES, OpeningOpts, PLAIN_HINT, TAGLINE, actions};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};

/// The card's full width.
const WIDTH: u16 = 47;
/// The sprocket rows' width (two leading spaces, 15 two-cell holes).
const SPROCKETS_W: u16 = 46;
/// The narrowest framed card: the tagline plus its three-cell margins.
const MIN_FRAMED: u16 = 42;
/// The art title's three rows.
const TITLE_ART: [&str; 3] = [
    "█▀▄ ▄▀▄ ▄▀▀ █▄▀ █   ▄▀▄ ▀█▀",
    "█▀▄ █▀█ █   █▀▄ █   █ █  █",
    "▀▀  ▀ ▀  ▀▀ ▀ ▀ ▀▀▀  ▀   ▀",
];
/// The plain title the art falls back to.
const TITLE_TEXT: &str = "B A C K L O T";

/// Where the parts go, or `None` when even the frame does not fit.
struct Layout {
    block_w: u16,
    card_w: u16,
    card_h: u16,
    art: bool,
    sprockets: bool,
    x: u16,
    y: u16,
}

impl Layout {
    fn total_h(&self) -> u16 {
        self.card_h + if self.sprockets { 2 } else { 0 } + 1 // the footer
    }
}

/// Choose the rung of the ladder that fits `area`.
fn layout(area: Rect, rows: usize) -> Option<Layout> {
    let n = rows as u16;
    let art_h = 10 + n; // frame, blank, 3 title rows, blank, tagline,
    // rule, actions, blank, frame
    let text_h = 8 + n; // the same with one title row
    let (card_w, card_h, art, sprockets) = if area.width >= WIDTH && area.height >= art_h + 3 {
        (WIDTH, art_h, true, true)
    } else if area.width >= WIDTH && area.height > art_h {
        (WIDTH, art_h, true, false)
    } else if area.width >= SPROCKETS_W && area.height >= text_h + 3 {
        (WIDTH.min(area.width), text_h, false, true)
    } else if area.width >= MIN_FRAMED && area.height > text_h {
        (WIDTH.min(area.width), text_h, false, false)
    } else {
        return None;
    };
    let block_w = card_w.max(if sprockets { SPROCKETS_W } else { 0 });
    let total = card_h + if sprockets { 2 } else { 0 } + 1;
    Some(Layout {
        block_w,
        card_w,
        card_h,
        art,
        sprockets,
        x: area.x + area.width.saturating_sub(block_w) / 2,
        y: area.y + area.height.saturating_sub(total) / 2,
    })
}

/// Paint the card. `true` when it drew; `false` sends the caller on to
/// the plain layout.
pub(super) fn render(
    buffer: &mut Buffer,
    area: Rect,
    palette: &Palette,
    opts: &OpeningOpts,
    frame: Option<u8>,
) -> bool {
    let rows = actions(opts);
    let Some(layout) = layout(area, rows.len()) else {
        return false;
    };
    let card_top = layout.y + u16::from(layout.sprockets);
    let card = Rect::new(layout.x, card_top, layout.card_w, layout.card_h);
    buffer.set_style(card, Style::default().bg(palette.surface_dim));
    draw_frame(buffer, card, palette);
    if layout.sprockets {
        draw_sprockets(buffer, &layout, palette, frame);
    }
    let inner = Rect::new(
        card.x + 1,
        card.y + 1,
        card.width.saturating_sub(2),
        layout.card_h.saturating_sub(2),
    );
    draw_content(buffer, inner, palette, &rows, layout.art, frame);
    draw_footer(buffer, &layout, palette);
    true
}

fn draw_frame(buffer: &mut Buffer, card: Rect, palette: &Palette) {
    if card.width < 2 || card.height < 2 {
        return;
    }
    let style = Style::default().fg(palette.accent).bg(palette.surface_dim);
    let inner = card.width - 2;
    put_text(
        buffer,
        card.x,
        card.y,
        card.width,
        &format!("╔{}╗", "═".repeat(inner as usize)),
        style,
    );
    for row in 1..card.height.saturating_sub(1) {
        put_text(buffer, card.x, card.y + row, 1, "║", style);
        put_text(buffer, card.right() - 1, card.y + row, 1, "║", style);
    }
    put_text(
        buffer,
        card.x,
        card.bottom() - 1,
        card.width,
        &format!("╚{}╝", "═".repeat(inner as usize)),
        style,
    );
}

/// The title's colour as the run fades it in: dim in the first third,
/// brighter in the second, then the accent in bold.
fn title_style(frame: Option<u8>, palette: &Palette) -> Style {
    let third = OPENING_FRAMES / 3;
    match frame {
        Some(f) if f < third => Style::default().fg(palette.surface1),
        Some(f) if f < third * 2 => Style::default().fg(palette.overlay1),
        _ => Style::default()
            .fg(palette.accent)
            .add_modifier(Modifier::BOLD),
    }
}

fn centered_x(inner: Rect, width: u16) -> u16 {
    inner.x + inner.width.saturating_sub(width) / 2
}

fn draw_content(
    buffer: &mut Buffer,
    inner: Rect,
    palette: &Palette,
    rows: &[Action],
    art: bool,
    frame: Option<u8>,
) {
    let title = title_style(frame, palette);
    let mut row = inner.y + 1; // row 0 is the blank under the frame
    if art {
        for line in TITLE_ART {
            let width = (display_width(line) as u16).min(inner.width);
            put_text(buffer, centered_x(inner, width), row, width, line, title);
            row += 1;
        }
    } else {
        let width = (display_width(TITLE_TEXT) as u16).min(inner.width);
        put_text(
            buffer,
            centered_x(inner, width),
            row,
            width,
            &truncate_end(TITLE_TEXT, width as usize),
            title,
        );
        row += 1;
    }
    row += 1; // the blank under the title

    let tagline_w = (display_width(TAGLINE) as u16).min(inner.width);
    put_text(
        buffer,
        centered_x(inner, tagline_w),
        row,
        tagline_w,
        &truncate_end(TAGLINE, tagline_w as usize),
        Style::default().fg(palette.subtext0),
    );
    row += 1;

    // The rule: three cells of air each side, the rest the line.
    let rule_w = inner.width.saturating_sub(6);
    if rule_w > 0 {
        put_text(
            buffer,
            inner.x + 3,
            row,
            rule_w,
            &"─".repeat(rule_w as usize),
            Style::default().fg(palette.surface1),
        );
    }
    row += 1;

    let indent = 7.min(inner.width.saturating_sub(1));
    let key_w = rows.iter().map(|a| a.key.len()).max().unwrap_or(1) as u16;
    for action in rows {
        if row >= inner.bottom() {
            break;
        }
        put_text(
            buffer,
            inner.x + indent,
            row,
            key_w,
            action.key,
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        );
        let label_x = inner.x + indent + key_w + 3;
        let label_w = inner.right().saturating_sub(label_x);
        if label_w > 0 {
            put_text(
                buffer,
                label_x,
                row,
                label_w,
                &truncate_end(action.label, label_w as usize),
                Style::default().fg(palette.text),
            );
        }
        row += 1;
    }
}

/// Both sprocket rows, shifted one column per frame like film running
/// through a gate. The last frame is the static page.
fn draw_sprockets(buffer: &mut Buffer, layout: &Layout, palette: &Palette, frame: Option<u8>) {
    let shift = match frame {
        Some(f) if f + 1 < OPENING_FRAMES => (f as usize + 1) % 3,
        _ => 0,
    };
    let style = Style::default().fg(palette.surface1).bg(palette.panel_bg);
    for (row, glyph) in [(layout.y, '▄'), (layout.y + layout.card_h + 1, '▀')] {
        let line = shift_left(&sprocket_row(glyph), shift);
        let width = (display_width(&line) as u16).min(layout.block_w);
        put_text(buffer, layout.x, row, width, &line, style);
    }
}

/// `  ` plus 15 two-cell holes, one cell between them: 46 columns.
fn sprocket_row(glyph: char) -> String {
    let mut line = String::from("  ");
    for i in 0..15 {
        if i > 0 {
            line.push(' ');
        }
        line.push(glyph);
        line.push(glyph);
    }
    line
}

/// Rotate the strip left by `shift` columns.
fn shift_left(line: &str, shift: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let shift = shift % chars.len();
    chars[shift..].iter().chain(chars[..shift].iter()).collect()
}

fn draw_footer(buffer: &mut Buffer, layout: &Layout, palette: &Palette) {
    let line = format!(" v{} · {}", env!("CARGO_PKG_VERSION"), PLAIN_HINT);
    let row = layout.y + layout.total_h() - 1;
    put_text(
        buffer,
        layout.x,
        row,
        layout.block_w,
        &truncate_end(&line, layout.block_w as usize),
        Style::default().fg(palette.overlay0),
    );
}

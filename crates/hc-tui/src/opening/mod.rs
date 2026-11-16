//! The opening page (U2): what backlot shows in the pane area while
//! no studio exists — no `backlot` document, or a good one with zero
//! studios — and while `new_workspace` has called it up. A welcome
//! screen: what backlot is in one line, the actions with their keys,
//! and a dim pointer to `backlot --plain`. Palette colours only,
//! centred, readable at 40 columns. The keys it owns live in
//! `app/opening.rs`; the state is memory only — nothing is stored.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};

/// The client-local page state: Esc dismisses for the session,
/// `new_workspace` brings it back. Nothing is stored.
#[derive(Debug, Default)]
pub(crate) struct Opening {
    /// Esc dismissed the page until `new_workspace` asks for it.
    pub dismissed: bool,
    /// `new_workspace` asked for the page even over a live studio.
    pub forced: bool,
}

/// What the page needs from the app, so tests can draw it without
/// one: `[hollywood]` gates `h`, a live stage terminal gates `s`.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct OpeningOpts {
    /// A `[hollywood]` table configures the overseer.
    pub hollywood: bool,
    /// A `backlot-stage` terminal is live on this core.
    pub stage_running: bool,
}

/// One action row's key and label.
struct Action {
    key: &'static str,
    label: &'static str,
}

/// The rows for `opts`, in page order.
fn actions(opts: &OpeningOpts) -> Vec<Action> {
    let mut rows = vec![Action {
        key: "n",
        label: "New studio",
    }];
    if opts.hollywood {
        rows.push(Action {
            key: "h",
            label: "Tell Hollywood",
        });
    }
    if !opts.stage_running {
        rows.push(Action {
            key: "s",
            label: "Start the stage",
        });
    }
    rows.push(Action {
        key: "q",
        label: "detach",
    });
    rows
}

/// The one-line description — fits 40 columns.
const TAGLINE: &str = "a film studio for coding agents";
/// The dim last line — names the plain multiplexer. Secondary text:
/// `surface_dim` is a separator colour a step off the panel
/// background, so it drew this line nearly invisible.
const PLAIN_HINT: &str = "backlot --plain — a plain multiplexer";

/// Paint the page over `area` (the pane surface). Content keeps to a
/// ~40-column block, centred horizontally and vertically; narrower
/// frames clip lines on the right, never panic.
pub(crate) fn render_opening(
    buffer: &mut Buffer,
    area: Rect,
    palette: &Palette,
    opts: OpeningOpts,
) {
    if area.is_empty() {
        return;
    }
    buffer.set_style(area, Style::default().bg(palette.panel_bg));

    let rows = actions(&opts);
    let key_col = rows.iter().map(|a| a.key.len()).max().unwrap_or(1) as u16;
    let label_max = rows
        .iter()
        .map(|a| display_width(a.label) as u16)
        .max()
        .unwrap_or(0);
    // Block width: the longest row the page can show, capped at 40
    // columns of content so a wide frame keeps it a page, not a span.
    let block = (display_width(PLAIN_HINT) as u16)
        .max(key_col + 2 + label_max)
        .min(40)
        .min(area.width);
    let height = (rows.len() + 4) as u16; // tagline, gap, rows, gap, hint
    let x = area.x + area.width.saturating_sub(block) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;

    put_text(
        buffer,
        x,
        y,
        block,
        &truncate_end(TAGLINE, block as usize),
        Style::default().fg(palette.subtext0),
    );
    let rows = actions(&opts);
    for (i, action) in rows.iter().enumerate() {
        let row = y + 2 + i as u16;
        if row >= area.bottom() {
            break;
        }
        put_text(
            buffer,
            x,
            row,
            key_col,
            action.key,
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        );
        put_text(
            buffer,
            x + key_col + 2,
            row,
            block.saturating_sub(key_col + 2),
            &truncate_end(action.label, block.saturating_sub(key_col + 2) as usize),
            Style::default().fg(palette.text),
        );
    }
    let hint_row = y + height - 1;
    if hint_row < area.bottom() {
        put_text(
            buffer,
            x,
            hint_row,
            block,
            &truncate_end(PLAIN_HINT, block as usize),
            Style::default().fg(palette.overlay0),
        );
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

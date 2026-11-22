//! The opening page (U2/U4): what backlot shows in the pane area while
//! no studio exists — no `backlot` document, or a good one with zero
//! studios — and while `new_workspace` has called it up. A film-strip
//! title card: sprocket rows, a double frame lit like a screen, the
//! block title, a tagline and the actions with their keys. Palette
//! colours only, centred, readable down to 40 columns. The keys it
//! owns live in `app/opening.rs`; the state is memory only.
//!
//! The card and its fallbacks live in `opening/card.rs`; the plain
//! layout the ladder ends on is here.

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};

mod card;

/// One animation frame of the one-time opening run.
pub(crate) const FRAME: Duration = Duration::from_millis(80);
/// Frames in the run (15 × 80 ms ≈ 1.2 s); the last frame is the
/// static page.
pub(crate) const OPENING_FRAMES: u8 = 15;

/// How long the entry decision waits for the first `backlot` read
/// (U7). A client with studios must never flash the welcome card
/// while the first read is on the wire; after this cap the client
/// judges with what it has.
pub(crate) const SETTLE_CAP: Duration = Duration::from_secs(2);

/// The client-local page state: Esc dismisses for the session,
/// `new_workspace` brings it back. Nothing is stored.
#[derive(Debug, Default)]
pub(crate) struct Opening {
    /// Esc dismissed the page until `new_workspace` asks for it.
    pub dismissed: bool,
    /// The first `backlot` read has landed — or [`SETTLE_CAP`] passed.
    pub settled: bool,
    /// The cap's deadline, armed at the first decision pass and
    /// cleared once settled, so nothing waits on a timer after entry.
    pub cap: Option<std::time::Instant>,
    /// The one-time animation: `done` after it played (or was left),
    /// so it never replays on a resize, Esc or any later redraw.
    pub anim: Anim,
}

impl Opening {
    /// The entry decision is made: the card or the layer may show.
    pub fn ready(&self) -> bool {
        self.settled
    }
}

/// How far the one-time opening animation has come.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Anim {
    /// The frame being drawn, 0-based.
    pub frame: u8,
    /// The run finished — or the page was left before it did.
    pub done: bool,
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
pub(super) struct Action {
    pub(super) key: &'static str,
    pub(super) label: &'static str,
}

/// The rows for `opts`, in page order.
pub(super) fn actions(opts: &OpeningOpts) -> Vec<Action> {
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

/// The one-line description.
pub(super) const TAGLINE: &str = "a film studio for coding agents";
/// The dim last line — how plain mode starts; a note, never a row
/// that reads like an in-page action (R3-M13).
pub(super) const PLAIN_HINT: &str = "plain: backlot --plain";

/// Paint the page over `area` (the pane surface). `frame` is the
/// one-time animation's frame, or `None` for the static page.
pub(crate) fn render_opening(
    buffer: &mut Buffer,
    area: Rect,
    palette: &Palette,
    opts: OpeningOpts,
    frame: Option<u8>,
) {
    if area.is_empty() {
        return;
    }
    buffer.set_style(area, Style::default().bg(palette.panel_bg));
    if card::render(buffer, area, palette, &opts, frame) {
        return;
    }
    render_plain(buffer, area, palette, &opts);
}

/// Today's plain layout — the ladder's last fallback, and what a
/// one-cell area gets: tagline, actions, dim hint. Narrow frames clip
/// lines on the right, never panic.
fn render_plain(buffer: &mut Buffer, area: Rect, palette: &Palette, opts: &OpeningOpts) {
    let rows = actions(opts);
    let key_col = rows.iter().map(|a| a.key.len()).max().unwrap_or(1) as u16;
    let label_max = rows
        .iter()
        .map(|a| display_width(a.label) as u16)
        .max()
        .unwrap_or(0);
    let block = (display_width(TAGLINE) as u16)
        .max(display_width(PLAIN_HINT) as u16)
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

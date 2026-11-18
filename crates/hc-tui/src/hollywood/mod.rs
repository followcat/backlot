//! The Hollywood layer (H4): a client-side floating view of the
//! `backlot` summary document's `hollywood` object — hidden, the lot
//! pill's collapsed face (the chrome draws it), or a band that drops
//! down from the top. It never reserves columns or rows: the pane
//! layout is never recomputed for it and no program is resized. All
//! the data it shows comes from the document; everything it changes
//! goes through `backlot` commands spawned off the event loop.
//!
//! `--plain` keeps the layer `Hidden`: no cells, no consumed key, no
//! timer — the frame is byte-identical to herdr (D15, D6).

mod actions;
mod band;
mod chips;
mod draw;
mod frames;
mod items;
mod keys;
mod new_frame;
mod plan;
mod rows;
mod sheet;
mod title;
mod view;

pub(crate) use actions::{HollywoodAction, HollywoodRunner, spawn_runner};
pub(crate) use draw::{HollywoodRender, expanded_height, render};
pub(crate) use keys::{CallSheetKeys, resolve_toggle};
pub(crate) use rows::{Target, band_rows, frames};
pub(crate) use sheet::{status_of, waiting_items};
pub(crate) use view::CardView;

use crate::chrome::text_editor::TextEditor;

/// How the layer sits on screen. `Hidden` draws nothing — the
/// default, and the only state when `[hollywood]` is absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LayerMode {
    /// Not drawn; no key is consumed; nothing is armed.
    #[default]
    Hidden,
    /// One row at the right end of the top row, sized to its content.
    Collapsed,
    /// The drop-down panel — the scrollable view with the input line.
    Expanded,
}

/// What the renderer needs from the app: whether a `[hollywood]` table
/// configures the agent, and the narrow-width cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct LayerOpts {
    /// An agent is configured: the headline, the cards, the `tell
    /// hollywood` line and the animated role state belong to it.
    /// Without one the bar still exists, in its no-agent form (U7).
    pub agent: bool,
    /// The narrow-width full-screen cut.
    pub mobile: bool,
}

/// The whole client-local layer state (D21): which of the three
/// states it is in, the selection, the frame window, the card scroll
/// and the input line's text. Everything on screen is read from the
/// document.
pub(crate) struct Hollywood {
    pub mode: LayerMode,
    /// The selected frame or row's stable identity — frames and rows
    /// rebuild each frame, so selection survives document updates by
    /// target, not index.
    pub selected: Option<Target>,
    /// The first frame drawn: the frames scroll sideways when more
    /// exist than fit.
    pub frame_scroll: usize,
    /// First card row drawn.
    pub scroll: usize,
    /// The input line's editor — `editing` is the latch that sends it
    /// keys (`i` or a click opens it, `Esc` leaves it).
    pub input: TextEditor,
    pub editing: bool,
    /// A card's "Show me" read — `card show <id>` output in a
    /// read-only box over the layer.
    pub view: Option<CardView>,
    /// The armed one-key confirm — S1's inline `finish` ask.
    pub confirm: Option<Confirm>,
    /// The running action's row label — the row draws `…` until the
    /// result lands.
    pub pending: Option<String>,
}

/// The inline `y/n` confirm S1 arms on a ready row: which studio
/// `f` would finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Confirm {
    /// `backlot studio finish` on this studio — `y` runs it.
    Finish(crate::studio::StudioKey),
}

impl Hollywood {
    pub fn new() -> Hollywood {
        Hollywood {
            mode: LayerMode::Hidden,
            selected: None,
            frame_scroll: 0,
            scroll: 0,
            input: TextEditor::new("", false),
            editing: false,
            view: None,
            confirm: None,
            pending: None,
        }
    }

    /// Drawn at all — the pill face counts.
    pub fn visible(&self) -> bool {
        self.mode != LayerMode::Hidden
    }

    /// The expanded panel owns its keys like the studio panel does.
    pub fn expanded(&self) -> bool {
        self.mode == LayerMode::Expanded
    }
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;

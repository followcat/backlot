//! The Hollywood layer (H4): a client-side floating view of the
//! `backlot` summary document's `hollywood` object — hidden, a
//! collapsed strip at the top row's right end, or a panel that drops
//! down from the top. It never reserves columns or rows: the pane
//! layout is never recomputed for it and no program is resized. All
//! the data it shows comes from the document; everything it changes
//! goes through `backlot` commands spawned off the event loop.
//!
//! With no `[hollywood]` in the config the layer stays `Hidden`: no
//! cells, no consumed key, no timer — the frame is byte-identical to
//! before the layer existed (D15, D6).

mod actions;
mod draw;
mod keys;
mod rows;
mod view;

pub(crate) use actions::{HollywoodAction, HollywoodRunner, spawn_runner};
pub(crate) use draw::{BtnKind, CardBtn, HollywoodRender, render};
pub(crate) use keys::{HollywoodKey, resolve_key};
pub(crate) use rows::{Row, Target, rows, strip_text};
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
/// states it is in, the selection, the scroll offset and the input
/// line's text. Everything on screen is read from the document.
pub(crate) struct Hollywood {
    pub mode: LayerMode,
    /// The selected row's stable identity — rows rebuild each frame,
    /// so selection survives document updates by target, not index.
    pub selected: Option<Target>,
    /// First body row rendered under the title.
    pub scroll: usize,
    /// The input line's editor — `editing` is the latch that sends it
    /// keys (`i` or a click opens it, `Esc` leaves it).
    pub input: TextEditor,
    pub editing: bool,
    /// A card's "Show me" read — `card show <id>` output in a
    /// read-only box over the layer.
    pub view: Option<CardView>,
}

impl Hollywood {
    pub fn new() -> Hollywood {
        Hollywood {
            mode: LayerMode::Hidden,
            selected: None,
            scroll: 0,
            input: TextEditor::new("", false),
            editing: false,
            view: None,
        }
    }

    /// Drawn at all — the strip or the panel.
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

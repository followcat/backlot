//! The studio panel (S4b): a client-side view of the `backlot`
//! summary document, drawn as a right-hand column or a full-screen
//! view. All panel state is client-local memory — nothing here writes
//! to the core's documents or changes hc core behavior. With `mode ==
//! Closed` the client is cell-identical to herdr: no columns are
//! reserved, no keys are consumed and no timer is armed.

mod actions;
mod config;
mod data;
mod draw;
mod rows;

pub(crate) use actions::{PanelAction, PanelRunner, spawn_runner};
pub(crate) use config::{PanelConfig, WIDTH_MAX, WIDTH_MIN, config_path, load};
pub(crate) use data::{stage_running, summary};
pub(crate) use draw::{PanelRender, render};
pub(crate) use rows::{Mark, Target, mark_color, mark_symbol, rows, title_text};

/// How the panel sits on screen. The default is `Closed`: no state,
/// no width, no keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PanelMode {
    /// Not drawn; zero width; the frame is herdr's.
    #[default]
    Closed,
    /// A 16-column strip naming the working groups.
    Collapsed,
    /// The full panel at the configured width.
    Expanded,
}

/// The whole client-local panel state.
pub(crate) struct Panel {
    pub mode: PanelMode,
    /// Whether panel keys/mouse are owned by the panel — a separate
    /// latch from `Mode`, so no new terminal mode exists.
    pub focused: bool,
    /// `(studio, group)` role groups collapsed by hand — kept across
    /// document updates, like the sidebar's collapse sets.
    pub collapsed_groups: std::collections::BTreeSet<(String, String)>,
    /// First row index rendered under the title.
    pub scroll: usize,
    /// The selected row's stable identity — rows rebuild each frame,
    /// so selection survives document updates by target, not index.
    pub selected: Option<Target>,
    /// A closed person whose reopen is one more Enter away.
    pub reopen_confirm: Option<(String, String)>,
    /// Spinner frame counter for working marks (~120 ms ticks).
    pub spin: usize,
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;

impl Panel {
    pub fn new() -> Panel {
        Panel {
            mode: PanelMode::Closed,
            focused: false,
            collapsed_groups: Default::default(),
            scroll: 0,
            selected: None,
            reopen_confirm: None,
            spin: 0,
        }
    }

    /// Drawn at all — beside the panes or full-screen.
    pub fn open(&self) -> bool {
        self.mode != PanelMode::Closed
    }
}

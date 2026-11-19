//! The studio panel: a client-side view of the `backlot` summary
//! document — one bordered block per studio in a narrow right-hand
//! column (U1) or a full-screen view. All panel state is client-local
//! memory — nothing here writes to the core's documents or changes hc
//! core behavior. With `mode == Closed` the client is cell-identical
//! to herdr: no columns are reserved, no keys are consumed and no
//! timer is armed.

mod config;
pub(crate) mod data;
mod draw;
mod draw_blocks;
mod draw_cards;
mod draw_tasks;
mod pending;
pub(crate) mod remote;
mod row_lines;
mod rows;
mod rows_fit;

pub(crate) use config::{
    CallSheetStart, PanelConfig, StudioKeys, StudioNav, WIDTH_MAX, WIDTH_MIN, config_path, load,
};
pub(crate) use data::{key as document_key, stage_running};
pub(crate) use draw::{PanelRender, render};
pub(crate) use remote::RemoteRunner;
pub(crate) use row_lines::owed_text;
pub(crate) use rows::{Row, Target, rows};
pub(crate) use rows_fit::mark_counts;

/// How the panel sits on screen. The default is `Closed`: no state,
/// no width, no keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PanelMode {
    /// Not drawn; zero width; the frame is herdr's.
    #[default]
    Closed,
    /// A 16-column strip with one line per studio.
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
    /// Studios `Space` collapsed by hand — overrides both the
    /// everything-fits expansion and the current studio.
    pub collapsed_blocks: std::collections::BTreeSet<crate::studio::StudioKey>,
    /// Studios `Space` expanded by hand while stacking — overrides
    /// the not-everything-fits collapse, not a manual collapse.
    pub expanded_blocks: std::collections::BTreeSet<crate::studio::StudioKey>,
    /// First row index rendered under the title.
    pub scroll: usize,
    /// The selected row's stable identity — rows rebuild each frame,
    /// so selection survives document updates by target, not index.
    pub selected: Option<Target>,
    /// The row `selected` last resolved to — a block's several
    /// `Studio` rows share one target, so the index pins which
    /// occurrence the selection actually sits on. A stale pin
    /// (rows shifted by a publish) degrades to the nearest
    /// surviving occurrence, never to position 0.
    pub selected_row: Option<usize>,
    /// The `Space` drawer's owner row — a `Person` or `Task` target;
    /// the three drawer lines draw under it (S2 §3).
    pub drawer: Option<Target>,
    /// Studios whose `✓ <n> accepted` fold is open — the accepted
    /// task rows show instead of the fold line.
    pub accepted_folds: std::collections::BTreeSet<crate::studio::StudioKey>,
    /// `f` on a ready-to-finish studio armed the inline
    /// `finish <id>? [y/n]` confirm — `y` runs `studio finish`, `n`
    /// or `Esc` stands down (S2 §2's `[f]` hint made real).
    pub finish_confirm: Option<crate::studio::StudioKey>,
    /// Spinner frame counter for working marks (~120 ms ticks).
    pub spin: usize,
    /// The width the dock's border drag last left — the session's
    /// manual override of the `[dock] width`/tier (S5 §2).
    pub drag_width: Option<u16>,
    /// H4: the expanded panel's last row names `backlot hollywood
    /// setup` while no `[hollywood]` table exists (R22). Set by
    /// `set_hollywood` — a client-local flag, never read from a doc.
    pub hollywood_hint: bool,
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;

impl Panel {
    pub fn new() -> Panel {
        Panel {
            mode: PanelMode::Closed,
            focused: false,
            collapsed_blocks: Default::default(),
            expanded_blocks: Default::default(),
            scroll: 0,
            selected: None,
            selected_row: None,
            drawer: None,
            accepted_folds: Default::default(),
            finish_confirm: None,
            spin: 0,
            drag_width: None,
            // The flag stays off until `set_hollywood` reads the
            // config — panel-only construction draws no hint.
            hollywood_hint: false,
        }
    }

    /// Drawn at all — beside the panes or full-screen.
    pub fn open(&self) -> bool {
        self.mode != PanelMode::Closed
    }

    /// Whether `key`'s accepted fold is open — the accepted task
    /// rows draw instead of the `✓ <n> accepted` line.
    pub(crate) fn accepted_open(&self, key: &crate::studio::StudioKey) -> bool {
        self.accepted_folds.contains(key)
    }
}

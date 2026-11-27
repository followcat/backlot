//! The studio panel: a client-side view of the `backlot` summary
//! document — one bordered block per studio in a narrow right-hand
//! column (U1) or a full-screen view. All panel state is client-local
//! memory — nothing here writes to the core's documents or changes hc
//! core behavior. With `mode == Closed` the client is cell-identical
//! to herdr: no columns are reserved, no keys are consumed and no
//! timer is armed.

mod config;
pub(crate) mod chips;
pub(crate) mod data;
mod draw;
mod draw_blocks;
mod draw_cards;
mod draw_compact;
mod draw_design;
mod draw_hints;
mod draw_tasks;
mod pending;
pub(crate) mod remote;
mod row_lines;
mod rows;
mod rows_fit;
mod rows_fold;
mod update;

pub(crate) use config::{
    CallSheetStart, PanelConfig, StudioKeys, StudioNav, WIDTH_MAX, WIDTH_MIN, config_path, load,
};
pub(crate) use data::{key as document_key, stage_running};
pub(crate) use draw::{PanelRender, render};
pub(crate) use remote::RemoteRunner;
pub(crate) use row_lines::{info_line, owed_text};
pub(crate) use rows::{Row, StoppedLedger, Target, rows};
pub(crate) use rows_fit::{health_of, mark_counts};
pub(crate) use update::{UpdateHint, newest};

/// How the panel sits on screen. The default is `Expanded`: the
/// dock opens on its column beside the panes. A window too narrow
/// for panes+dock resolves the column to the collapsed strip for
/// that width — `panel.mode` keeps the chosen mode; the drawn mode
/// is `App::panel_mode_at(cols)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PanelMode {
    /// Not drawn; zero width; the frame is herdr's.
    Closed,
    /// A 16-column strip with one line per studio.
    Collapsed,
    /// The full panel at the configured width.
    #[default]
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
    /// Studios whose `✓ <n> done` fold is open — the accepted
    /// task rows show instead of the fold line.
    pub accepted_folds: std::collections::BTreeSet<crate::studio::StudioKey>,
    /// Studios whose `□ <n> closed` fold is open — the closed
    /// role rows show under the fold line.
    pub closed_folds: std::collections::BTreeSet<crate::studio::StudioKey>,
    /// Mirror of `shell_state.sidebar_collapsed` for this frame: while the
    /// left sidebar is open, the current studio card summarizes calm/standby
    /// roles instead of repeating Cast; when collapsed, all roles expand here.
    pub sidebar_collapsed: bool,
    /// `f` on a ready-to-finish studio armed the inline
    /// `finish <id>? [y/n]` confirm — `y` runs `studio finish`, `n`
    /// or `Esc` stands down (S2 §2's `[f]` hint made real).
    pub finish_confirm: Option<crate::studio::StudioKey>,
    /// Every studio's `›` line draft, keyed by its `StudioKey` —
    /// typing in `acme-shop` never paints into `beta-tools`' row.
    /// Resting on the row keeps the draft for the next `Compose`.
    pub drafts:
        std::collections::HashMap<crate::studio::StudioKey, crate::chrome::text_editor::TextEditor>,
    /// Per-studio `›` input history seeded from `StudioSummary.exchange`
    /// plus local submits in this session.
    pub histories:
        std::collections::HashMap<crate::studio::StudioKey, crate::chrome::history::ComposeHistory>,
    /// Whether the `›` line owns the keys — `Compose` vs `Resting`
    /// on a `Target::Input` row. Only `Enter`/`i`/`/`/a click arms
    /// it; selection movement, `Esc`, `↑` and a send all clear it.
    pub editing: bool,
    /// Spinner frame counter for working marks (~120 ms ticks).
    pub spin: usize,
    /// The width the dock's border drag last left — the session's
    /// manual override of the `[dock] width`/tier (S5 §2).
    pub drag_width: Option<u16>,
    /// H4: the expanded panel's last row names `backlot hollywood
    /// setup` while no `[hollywood]` table exists (R22). Set by
    /// `set_hollywood` — a client-local flag, never read from a doc.
    pub hollywood_hint: bool,
    /// S2: a newer installed `backlot` than this client runs — the
    /// foot of the lot says the client must be relaunched. Derived
    /// from the local stage's published version and the version probe
    /// before each draw (`App::update_hint`).
    pub update: Option<UpdateHint>,
    /// W1b: what the empty lot reads while no document has a studio but
    /// a stopped machine's ledger lists some — `<n> studios on stopped
    /// stages — s to start`. Set by `sync_studio`, `None` once any
    /// document lists a studio.
    pub(crate) stopped: Option<StoppedLedger>,
    /// One-line start command when `backlot-cat` is present while the server
    /// terminal is gone.
    pub(crate) server_stopped_cmd: Option<String>,
    /// Studio currently entered via `FocusState` (`InsideStudio` / `RowActions` / `Compose`).
    pub entered_studio: Option<crate::studio::StudioKey>,
    /// Highlighted inline action chip index when `LotLevel::RowActions` is active.
    pub active_chip: Option<usize>,
    /// True while navigating across studios at `LotLevel::Studios` so cursoring
    /// onto another studio's rows does not eagerly promote into `InsideStudio`.
    pub lot_studios_browse: bool,
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;

impl Panel {
    pub fn new() -> Panel {
        Panel {
            // The dock is visible by default: expanded beside the
            // panes, the collapsed strip when they cannot fit —
            // `--plain` closes it for the plain multiplexer's frame.
            mode: PanelMode::Expanded,
            focused: false,
            collapsed_blocks: Default::default(),
            expanded_blocks: Default::default(),
            scroll: 0,
            selected: None,
            selected_row: None,
            drawer: None,
            accepted_folds: Default::default(),
            closed_folds: Default::default(),
            sidebar_collapsed: false,
            finish_confirm: None,
            drafts: std::collections::HashMap::new(),
            histories: std::collections::HashMap::new(),
            editing: false,
            spin: 0,
            drag_width: None,
            // The flag stays off until `set_hollywood` reads the
            // config — panel-only construction draws no hint.
            hollywood_hint: false,
            update: None,
            stopped: None,
            server_stopped_cmd: None,
            entered_studio: None,
            active_chip: None,
            lot_studios_browse: false,
        }
    }

    /// Drawn at all — beside the panes or full-screen.
    pub fn open(&self) -> bool {
        self.mode != PanelMode::Closed
    }

    /// Whether `key`'s accepted fold is open — the accepted task
    /// rows draw instead of the `✓ <n> done` line.
    pub(crate) fn accepted_open(&self, key: &crate::studio::StudioKey) -> bool {
        self.accepted_folds.contains(key)
    }

    /// Whether `key`'s closed fold is open — the closed role rows
    /// draw under the `□ <n> closed` line.
    pub(crate) fn closed_open(&self, key: &crate::studio::StudioKey) -> bool {
        self.closed_folds.contains(key)
    }

    /// The studio's parked `›` draft — `None` while it never typed.
    pub(crate) fn draft(
        &self,
        key: &crate::studio::StudioKey,
    ) -> Option<&crate::chrome::text_editor::TextEditor> {
        self.drafts.get(key)
    }

    /// The studio's draft for writing — created empty on first use.
    pub(crate) fn draft_mut(
        &mut self,
        key: &crate::studio::StudioKey,
    ) -> &mut crate::chrome::text_editor::TextEditor {
        self.drafts
            .entry(key.clone())
            .or_insert_with(|| crate::chrome::text_editor::TextEditor::new("", false))
    }

    /// Drop the studio's draft — a send already did its work.
    pub(crate) fn clear_draft(&mut self, key: &crate::studio::StudioKey) {
        self.drafts.remove(key);
    }

    /// Per-studio Compose history entry, created on first access.
    pub(crate) fn history_mut(
        &mut self,
        key: &crate::studio::StudioKey,
    ) -> &mut crate::chrome::history::ComposeHistory {
        self.histories.entry(key.clone()).or_default()
    }

    /// Sync every visible studio's history from `StudioSummary.exchange`.
    pub(crate) fn sync_histories(&mut self, machines: &[crate::panel::data::MachineSummary]) {
        for m in machines {
            for studio in &m.summary.studios {
                let key = crate::studio::StudioKey {
                    machine: m.machine.clone(),
                    id: studio.id.clone(),
                };
                self.history_mut(&key).sync(
                    studio
                        .exchange
                        .iter()
                        .map(|e| (e.seq, e.who.as_str(), e.text.as_str())),
                );
            }
        }
    }

    /// Reset active history navigation cursors/stashes across every studio.
    pub(crate) fn reset_history_nav(&mut self) {
        for history in self.histories.values_mut() {
            history.reset_nav();
        }
    }
}

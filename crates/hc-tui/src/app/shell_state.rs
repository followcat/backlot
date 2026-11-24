//! The chrome state `ShellRenderState` borrows each frame — scroll
//! offsets, collapse sets, reveal flags and the sidebar split. Split
//! out of `mod.rs` to keep it under the file-size cap.

use crate::chrome::snapshot::ClientEndpointId;

/// Per-frame mutable chrome state.
#[derive(Default)]
pub(crate) struct ShellState {
    pub collapsed_endpoints: std::collections::HashSet<ClientEndpointId>,
    pub collapsed_groups: std::collections::HashSet<String>,
    pub remote_collapsed_groups:
        std::collections::HashMap<ClientEndpointId, std::collections::HashSet<String>>,
    pub workspace_scroll: usize,
    pub agent_scroll: usize,
    pub tab_scroll: usize,
    pub reveal_focused_workspace: bool,
    pub reveal_focused_tab: bool,
    /// A frame should scroll the agents list to the focused agent row —
    /// armed by a focus change, consumed by whichever agents list draws.
    pub reveal_focused_agent: bool,
    pub sidebar_collapsed: bool,
    /// Expanded sidebar's spaces:agents split, `0.0..=1.0`.
    pub sidebar_section_split: f32,
    pub reveal_navigation_workspace: bool,
    /// W1: scroll the machines tree to the current studio's machine row
    /// (M4 — the row never falls out of a short sidebar).
    pub reveal_studio_row: bool,
    /// W1b: scroll the machines tree to the keyboard cursor's row.
    pub reveal_tree_row: bool,
    /// Mobile switcher viewport scroll (Navigate mode under
    /// `mobile_width_threshold` — the switcher is the mobile picker).
    pub mobile_switcher_scroll: usize,
    /// Scrolls the switcher to the pending workspace highlight on move.
    pub reveal_mobile_workspace: bool,
    pub machine_diagnostics: crate::chrome::render::MachineDiagnostics,
    /// Split/pane border cells keyed by the last frame's layout.
    pub border_cache: crate::chrome::panes_draw::PaneBorderCache,
}

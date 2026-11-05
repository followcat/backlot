// Derived from herdr src/layout.rs (Apache-2.0).
//! Pinned reference BSP geometry and navigation, independent of terminal I/O.

use std::cmp::Reverse;

use ratatui::{
    layout::{Direction, Rect},
    widgets::Borders,
};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct PaneId(u32);

/// Global atomic counter for unique PaneId generation across all workspaces.
static NEXT_PANE_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

impl PaneId {
    /// Allocate a globally unique PaneId.
    pub fn alloc() -> Self {
        Self(NEXT_PANE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }

    pub fn raw(self) -> u32 {
        self.0
    }

    /// Reconstruct from a saved u32 (persistence only).
    pub fn from_raw(id: u32) -> Self {
        Self(id)
    }
}

/// Snapshot of a pane's position and focus state after layout.
#[derive(Clone)]
pub struct PaneInfo {
    pub id: PaneId,
    /// Outer rect (including borders if present).
    pub rect: Rect,
    /// Inner rect (content area, excluding borders). Used for selection.
    pub inner_rect: Rect,
    /// Visible scrollbar lane, when scrollback is present. `inner_rect` may still
    /// exclude a stable hidden gutter when this is `None`.
    pub scrollbar_rect: Option<Rect>,
    /// Borders drawn around this pane after UI chrome is applied.
    pub borders: Borders,
    pub is_focused: bool,
}

/// Info about a split boundary, used for mouse drag resize.
#[derive(Clone)]
pub struct SplitBorder {
    /// Position of the divider line (x for horizontal split, y for vertical).
    pub pos: u16,
    /// Direction of the split that created this border.
    pub direction: Direction,
    /// Ratio assigned to the first child of this split.
    pub ratio: f32,
    /// Total area of the split node.
    pub area: Rect,
    /// Path from root to this split node (false=first, true=second).
    pub path: Vec<bool>,
}

/// Cardinal direction for pane navigation.
#[derive(Debug, Clone, Copy)]
pub enum NavDirection {
    Left,
    Right,
    Up,
    Down,
}

/// A node in the BSP tree. Public for serialization.
#[derive(Clone)]
pub enum Node {
    Pane(PaneId),
    Split {
        direction: Direction,
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

/// BSP tiling layout. Tracks a tree of splits and a focused pane.
#[derive(Clone)]
pub struct TileLayout {
    root: Node,
    focus: PaneId,
    /// Pane focused before `focus`, used by `close_focused`. Only a real focus
    /// move writes it; tree edits go through the target-taking primitives
    /// (`split_pane`, `close_pane`, unfocused `insert_pane_near`) so internal
    /// focus excursions never corrupt it.
    prev_focus: Option<PaneId>,
}

include!("tile.rs");
include!("navigation.rs");
include!("tree.rs");

#[cfg(test)]
mod tests;

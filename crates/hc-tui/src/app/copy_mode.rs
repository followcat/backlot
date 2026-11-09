// Derived from herdr src/client/shell/copy_mode.rs (Apache-2.0).
//! App-side copy mode state: entry and exit and focus/surface sync.
//! Cursor movement lives in `app/copy_move.rs`, search and key routing
//! in `app/copy_keys.rs`; the motion/search engines are local ports in
//! `copy_buffer.rs`/`copy_window.rs`.

use std::time::Instant;

use crate::copy_mode::{CopySearchDirection, PaneTextPoint};

use super::{App, Mode};

/// The reference's `ClientCopyModeState`.
pub(crate) struct CopyModeState {
    pub pane_id: String,
    /// The reference compares the pane's VT content revision; changes
    /// invalidate matches the way its server-side scan did.
    pub content_revision: u64,
    /// (inner width, inner height) at last frame — resize drops linewise
    /// anchors the way the reference does.
    pub geometry: (u16, u16),
    /// Entered on the alternate screen: resize/coordinate changes
    /// invalidate state as in the reference.
    pub alternate_screen_active: bool,
    /// Cursor in absolute screen-buffer coordinates.
    pub cursor: PaneTextPoint,
    pub offset_from_bottom: usize,
    pub max_offset_from_bottom: usize,
    pub entry_offset_from_bottom: usize,
    pub selection: Option<CopySelection>,
    pub search_prompt: Option<CopySearchPrompt>,
    pub search_query: String,
    pub search_direction: Option<CopySearchDirection>,
    pub search_matches: Vec<crate::copy_mode::PaneTextRange>,
    pub search_total: usize,
    pub search_current: Option<usize>,
    pub search_current_global: Option<usize>,
}

/// The reference's `ClientCopySelection`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CopySelection {
    Character { anchor: PaneTextPoint },
    Linewise { anchor_row: u32 },
}

/// The reference's `ClientCopySearchPrompt`.
pub(crate) struct CopySearchPrompt {
    pub direction: CopySearchDirection,
    pub query: crate::chrome::text_editor::TextEditor,
}

impl App {
    /// The reference's `copy_or_terminal_mode`: prefix keys that return
    /// to a pane go back into Copy when it is armed for that pane.
    pub(super) fn copy_or_terminal_mode(&self) -> Mode {
        self.copy_mode
            .as_ref()
            .filter(|copy_mode| self.focused.as_deref() == Some(copy_mode.pane_id.as_str()))
            .map_or(Mode::Normal, |_| Mode::Copy)
    }

    /// The reference's `enter_copy_mode`. The cursor starts where the
    /// pane's own cursor sits in the viewport, or the bottom-left cell
    /// when it is hidden or outside the viewport.
    pub(super) fn enter_copy_mode(&mut self) -> bool {
        let Some(pane_id) = self.focused.clone() else {
            return false;
        };
        if self
            .copy_mode
            .as_ref()
            .is_some_and(|copy_mode| copy_mode.pane_id == pane_id)
        {
            self.mode = Mode::Copy;
            return true;
        }
        if self.copy_mode.is_some() {
            self.exit_copy_mode(false);
        }
        let Some(hit) = self
            .hit_map
            .panes
            .iter()
            .find(|hit| hit.pane_id == pane_id)
            .cloned()
        else {
            return false;
        };
        let Some(metrics) = hit.scroll else {
            return false;
        };
        let viewport_top = metrics
            .max_offset_from_bottom
            .saturating_sub(metrics.offset_from_bottom)
            .min(u32::MAX as usize) as u32;
        let cursor = self
            .displayed_pane_mut(&pane_id)
            .and_then(|pane| pane.vt())
            .and_then(|vt| vt.frame(0).ok())
            .and_then(|frame| {
                let cursor = frame.cursor;
                (cursor.visible
                    && cursor.position.is_some_and(|(x, y)| {
                        x < hit.inner_rect.width && y < hit.inner_rect.height
                    }))
                .then(|| {
                    let (x, y) = cursor.position.unwrap_or_default();
                    PaneTextPoint {
                        row: viewport_top.saturating_add(u32::from(y)),
                        col: x,
                    }
                })
            })
            .unwrap_or(PaneTextPoint {
                row: viewport_top
                    .saturating_add(u32::from(hit.inner_rect.height.saturating_sub(1))),
                col: 0,
            });
        self.selection = None;
        self.stop_selection_autoscroll();
        self.selection_highlight_clear_deadline = None;
        let (content_revision, alternate_screen_active) = self
            .displayed_pane(&pane_id)
            .and_then(|pane| pane.vt_ref())
            .map_or((0, false), |vt| {
                (vt.content_revision(), vt.alt_screen().unwrap_or(false))
            });
        self.copy_mode = Some(CopyModeState {
            pane_id,
            content_revision,
            geometry: (hit.inner_rect.width, hit.inner_rect.height),
            alternate_screen_active,
            cursor,
            offset_from_bottom: metrics.offset_from_bottom,
            max_offset_from_bottom: metrics.max_offset_from_bottom,
            entry_offset_from_bottom: metrics.offset_from_bottom,
            selection: None,
            search_prompt: None,
            search_query: String::new(),
            search_direction: None,
            search_matches: Vec::new(),
            search_total: 0,
            search_current: None,
            search_current_global: None,
        });
        self.mode = Mode::Copy;
        true
    }

    /// The reference's `exit_copy_mode`. With `copy`, a live selection is
    /// copied; without a selection the current search match is promoted
    /// to one first. Either way the pane returns to the scroll offset it
    /// had on entry.
    pub(super) fn exit_copy_mode(&mut self, copy: bool) {
        let live_selection = self.selection.as_ref().is_some_and(|s| s.is_visible());
        if copy
            && !live_selection
            && let Some((pane_id, text_match)) = self.copy_mode.as_ref().and_then(|copy_mode| {
                copy_mode
                    .search_current
                    .and_then(|current| copy_mode.search_matches.get(current).copied())
                    .map(|text_match| (copy_mode.pane_id.clone(), text_match))
            })
        {
            self.selection = Some(crate::selection::Selection::absolute_range(
                pane_id,
                (text_match.start.row, text_match.start.col),
                (text_match.end.row, text_match.end.col),
            ));
        }
        let Some(copy_mode) = self.copy_mode.take() else {
            return;
        };
        if copy && self.selection.as_ref().is_some_and(|s| s.is_visible()) {
            self.request_selection_copy();
        }
        self.selection = None;
        self.selection_highlight_clear_deadline = None;
        self.pane_scroll_to_offset(&copy_mode.pane_id, copy_mode.entry_offset_from_bottom);
        self.mode = Mode::Normal;
        self.sched.mark_dirty(Instant::now());
    }

    /// The copy mode's pane hit — geometry and scroll metrics.
    pub(super) fn copy_hit(&self) -> Option<crate::chrome::render::PaneHit> {
        let copy_mode = self.copy_mode.as_ref()?;
        self.hit_map
            .panes
            .iter()
            .find(|hit| hit.pane_id == copy_mode.pane_id)
            .cloned()
    }

    /// The reference's `apply_active_snapshot` copy-mode arms. A missing
    /// pane unarms the mode entirely; an unfocused pane drops the mode's
    /// selection and returns to Normal; a refocused copy pane re-arms
    /// Copy mode (the prefix→navigate→cancel round trip).
    pub(super) fn sync_copy_focus(&mut self) {
        let Some(copy_pane_id) = self.copy_mode.as_ref().map(|c| c.pane_id.clone()) else {
            return;
        };
        let pane_exists = self
            .projection
            .snapshot
            .panes
            .iter()
            .any(|pane| pane.pane_id == copy_pane_id);
        let pane_focused = self.focused.as_deref() == Some(copy_pane_id.as_str());
        if !pane_exists {
            self.copy_mode = None;
            if self
                .selection
                .as_ref()
                .is_some_and(|s| s.pane_id == copy_pane_id)
            {
                self.selection = None;
                self.stop_selection_autoscroll();
                self.selection_highlight_clear_deadline = None;
            }
            if self.mode == Mode::Copy {
                self.mode = Mode::Normal;
            }
        } else if pane_focused {
            // The armed prefix is the reference's `ClientShellMode::Prefix`,
            // which `apply_active_snapshot` never lifts to Copy — re-arm
            // only after the prefix machine disarms.
            if self.mode == Mode::Normal && !self.prefix.armed() {
                self.mode = Mode::Copy;
            }
            if self.selection.is_none() {
                self.sync_copy_selection();
            }
        } else {
            if self
                .selection
                .as_ref()
                .is_some_and(|s| s.pane_id == copy_pane_id)
            {
                self.selection = None;
                self.stop_selection_autoscroll();
                self.selection_highlight_clear_deadline = None;
            }
            if self.mode == Mode::Copy {
                self.mode = Mode::Normal;
            }
        }
    }

    /// The reference's `apply_pane_surface` copy arm — called from the
    /// draw path each frame with this pane's inner rect and scroll
    /// metrics. Content revisions and resizes invalidate search matches;
    /// a resize also drops the selection.
    pub(super) fn sync_copy_surface(
        &mut self,
        pane_id: &str,
        inner_rect: ratatui::layout::Rect,
        scroll: Option<crate::chrome::scroll::ScrollMetrics>,
    ) {
        if self.copy_mode.as_ref().is_none_or(|c| c.pane_id != pane_id) {
            return;
        }
        let (content_revision, alternate_screen_active) = self
            .displayed_pane(pane_id)
            .and_then(|pane| pane.vt_ref())
            .map_or((0, false), |vt| {
                (vt.content_revision(), vt.alt_screen().unwrap_or(false))
            });
        let geometry = (inner_rect.width, inner_rect.height);
        let mut invalidate_selection = false;
        if let Some(copy_mode) = self.copy_mode.as_mut() {
            let coordinates_changed = copy_mode.geometry != geometry
                || copy_mode.alternate_screen_active != alternate_screen_active;
            if copy_mode.content_revision != content_revision || coordinates_changed {
                copy_mode.content_revision = content_revision;
                copy_mode.geometry = geometry;
                copy_mode.alternate_screen_active = alternate_screen_active;
                if coordinates_changed {
                    copy_mode.selection = None;
                    invalidate_selection = true;
                }
                copy_mode.search_matches.clear();
                copy_mode.search_total = 0;
                copy_mode.search_current = None;
                copy_mode.search_current_global = None;
            }
            // Scroll pushes resolve synchronously here, so the surface's
            // reported offset is always authoritative.
            if let Some(scroll) = scroll {
                copy_mode.offset_from_bottom = scroll.offset_from_bottom;
                copy_mode.max_offset_from_bottom = scroll.max_offset_from_bottom;
            }
        }
        if invalidate_selection
            && self
                .selection
                .as_ref()
                .is_some_and(|s| s.pane_id == pane_id)
        {
            self.selection = None;
            self.stop_selection_autoscroll();
            self.selection_highlight_clear_deadline = None;
        }
    }
}

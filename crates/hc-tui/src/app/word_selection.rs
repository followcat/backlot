// Derived from herdr src/client/shell/word_selection.rs (Apache-2.0).
//! The double-click word gesture. The reference resolves anchor and
//! cursor rows through `PaneSelectionRead` RPCs and tracks a generation +
//! pending row so remote latency can't queue motion; hc-tui reads the
//! same absolute screen rows from the pane's own displayed VT
//! synchronously, so the only retained state is the gesture itself.

use std::time::{Duration, Instant};

use super::App;
use crate::chrome::render::PaneHit;
use crate::selection::Selection;

/// A bare double-click's word highlight fades after 500 ms — the
/// reference's `selection_highlight_clear_deadline` window.
const WORD_HIGHLIGHT_CLEAR: Duration = Duration::from_millis(500);

/// The reference's `ClientWordSelection`.
#[derive(Debug)]
pub(super) struct WordSelection {
    pub pane_id: String,
    anchor: (u32, u16),
    anchor_bounds: Option<(u16, u16)>,
    cursor: (u32, u16),
    end_col: u16,
    cached_row: Option<(u32, String)>,
    pub dragged: bool,
    pub released: bool,
}

impl App {
    /// Double-click: the word gesture anchors on the click's absolute row
    /// and immediately selects the word under it.
    pub(super) fn request_word_selection(&mut self, hit: &PaneHit, viewport_row: u16, col: u16) {
        let row = crate::selection::absolute_row_for_viewport(viewport_row, hit.scroll);
        self.word_selection = Some(WordSelection {
            pane_id: hit.pane_id.clone(),
            anchor: (row, col),
            anchor_bounds: None,
            cursor: (row, col),
            end_col: hit.inner_rect.width.saturating_sub(1),
            cached_row: None,
            dragged: false,
            released: false,
        });
        self.update_word_selection();
    }

    fn cancel_word_selection(&mut self) {
        self.word_selection = None;
        self.selection = None;
        self.stop_selection_autoscroll();
    }

    /// One absolute screen row of the pane's displayed VT — the local
    /// `PaneSelectionRead` row the reference's word gesture resolves.
    fn word_row_text(&self, pane_id: &str, row: u32, end_col: u16) -> Option<String> {
        self.displayed_pane(pane_id)
            .and_then(|pane| pane.vt_ref())
            .and_then(|vt| vt.screen_row_text(row, end_col.saturating_add(1)).ok())
    }

    /// Extend the gesture to a new absolute cursor cell — one cached row
    /// read per row crossed.
    pub(super) fn drag_word_selection(&mut self, cursor: (u32, u16)) {
        let Some(gesture) = self.word_selection.as_mut() else {
            return;
        };
        if gesture.released || gesture.cursor == cursor {
            return;
        }
        gesture.cursor = cursor;
        gesture.dragged = true;
        self.update_word_selection();
    }

    /// `Up(Left)` during the word gesture — release resolves the final
    /// bounds, then copies per `copy_on_select`.
    pub(super) fn finish_word_selection(&mut self) {
        self.stop_selection_autoscroll();
        if let Some(gesture) = self.word_selection.as_mut() {
            gesture.released = true;
        }
        self.update_word_selection();
    }

    fn update_word_selection(&mut self) {
        // Anchor bounds come from the anchor row's text; the cursor row's
        // text is cached so drags read each visited row once.
        let (pane_id, anchor_row, end_col) = {
            let Some(gesture) = self.word_selection.as_ref() else {
                return;
            };
            (gesture.pane_id.clone(), gesture.anchor.0, gesture.end_col)
        };
        if self
            .word_selection
            .as_ref()
            .is_some_and(|gesture| gesture.anchor_bounds.is_none())
        {
            let Some(text) = self.word_row_text(&pane_id, anchor_row, end_col) else {
                self.cancel_word_selection();
                return;
            };
            let bounds = crate::word_select::word_bounds_at_column(
                &text,
                self.word_selection
                    .as_ref()
                    .map(|gesture| gesture.anchor.1)
                    .unwrap_or(0),
            );
            let Some(gesture) = self.word_selection.as_mut() else {
                return;
            };
            gesture.anchor_bounds = bounds;
            gesture.cached_row = Some((anchor_row, text));
            if bounds.is_none() {
                self.cancel_word_selection();
                return;
            }
        }
        let cursor_row = self
            .word_selection
            .as_ref()
            .map(|gesture| gesture.cursor.0)
            .unwrap_or(0);
        let cached = self.word_selection.as_ref().is_some_and(|gesture| {
            gesture
                .cached_row
                .as_ref()
                .is_some_and(|(row, _)| *row == cursor_row)
        });
        if !cached {
            let Some(text) = self.word_row_text(&pane_id, cursor_row, end_col) else {
                self.cancel_word_selection();
                return;
            };
            let Some(gesture) = self.word_selection.as_mut() else {
                return;
            };
            gesture.cached_row = Some((cursor_row, text));
        }
        let Some(gesture) = self.word_selection.as_ref() else {
            return;
        };
        let Some((anchor_start, anchor_end)) = gesture.anchor_bounds else {
            self.cancel_word_selection();
            return;
        };
        let Some((_, text)) = gesture
            .cached_row
            .as_ref()
            .filter(|(row, _)| *row == gesture.cursor.0)
        else {
            return;
        };
        let (start_col, end_col) =
            crate::word_select::word_bounds_at_column(text, gesture.cursor.1)
                .unwrap_or((gesture.cursor.1, gesture.cursor.1));
        let start = (gesture.anchor.0, anchor_start).min((gesture.cursor.0, start_col));
        let end = (gesture.anchor.0, anchor_end).max((gesture.cursor.0, end_col));
        let released = gesture.released;
        let dragged = gesture.dragged;
        self.selection = Some(Selection::absolute_range(
            gesture.pane_id.clone(),
            start,
            end,
        ));
        if released {
            if let Some(selection) = self.selection.as_mut() {
                selection.finish();
            }
            self.word_selection = None;
            if self.shell.copy_on_select {
                self.request_selection_copy();
                if dragged {
                    self.selection = None;
                } else {
                    self.selection_highlight_clear_deadline =
                        Some(Instant::now() + WORD_HIGHLIGHT_CLEAR);
                }
            }
        }
    }
}

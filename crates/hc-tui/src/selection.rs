// Derived from herdr src/selection.rs (Apache-2.0).
//! Text selection and clipboard support.
//!
//! Selection lifecycle:
//!   MouseDown in pane → anchor recorded (no visual yet)
//!   MouseDrag         → selection becomes active, cells highlighted
//!   MouseUp           → selection finalized; copied when `copy_on_select`
//!   Next click / key  → a retained selection is cleared
//!
//! Double-click selects a word; the caller decides whether to copy it
//! immediately.
//!
//! Rows are stored in screen-buffer coordinates instead of viewport-relative
//! coordinates. That keeps selection stable while the pane scrolls.
//! Clipboard writes go through the app's host-output sink as OSC 52; the
//! reference's native-clipboard/WSL fallbacks are out of scope here.

use ratatui::layout::Rect;

use crate::chrome::scroll::ScrollMetrics;

/// Current phase of a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Mouse is down but hasn't moved yet. If released without
    /// moving, this was just a click — no selection created.
    Anchored,
    /// Mouse has moved from the anchor point. Cells are being highlighted.
    Dragging,
    /// Mouse released after dragging. Selection is visible and the gesture is
    /// complete. Clipboard policy is owned by the caller.
    Done,
}

/// A text selection within a terminal pane.
#[derive(Debug, Clone)]
pub(crate) struct Selection<P = String> {
    /// Which pane the selection belongs to.
    pub pane_id: P,
    /// Anchor position in screen-buffer coordinates (row, col).
    anchor: (u32, u16),
    /// Current/final position in screen-buffer coordinates (row, col).
    cursor: (u32, u16),
    /// Selection phase.
    phase: Phase,
}

impl<P> Selection<P> {
    /// Start a potential selection. This records the anchor but doesn't
    /// make anything visible yet — the user might just be clicking.
    pub fn anchor(pane_id: P, viewport_row: u16, col: u16, metrics: Option<ScrollMetrics>) -> Self {
        let anchor = (absolute_row_for_viewport_row(viewport_row, metrics), col);
        Self {
            pane_id,
            anchor,
            cursor: anchor,
            phase: Phase::Anchored,
        }
    }

    /// A selection already spanning two absolute cells — the word-drag
    /// gesture's output, where bounds come pre-expanded.
    pub(crate) fn absolute_range(pane_id: P, anchor: (u32, u16), cursor: (u32, u16)) -> Self {
        Self {
            pane_id,
            anchor,
            cursor,
            phase: Phase::Dragging,
        }
    }

    /// Convert the anchor's absolute row and pane-relative column back to
    /// screen coordinates. Adds the pane origin before clamping so the
    /// returned (screen_row, screen_col) can be compared directly against
    /// mouse screen positions.
    pub fn anchor_screen_pos(
        &self,
        pane_inner: Rect,
        metrics: Option<ScrollMetrics>,
    ) -> (u16, u16) {
        let viewport_row = viewport_row_for_absolute_row(self.anchor.0, metrics);
        // Convert pane-relative to screen coordinates, then clamp.
        let row = (viewport_row.saturating_add(pane_inner.y)).clamp(
            pane_inner.y,
            pane_inner.y + pane_inner.height.saturating_sub(1),
        );
        let col = (self.anchor.1.saturating_add(pane_inner.x)).clamp(
            pane_inner.x,
            pane_inner.x + pane_inner.width.saturating_sub(1),
        );
        (row, col)
    }

    /// Extend the selection as the mouse drags. Activates highlighting
    /// once the cursor moves to a different cell than the anchor.
    /// Screen coordinates are clamped to the pane boundary.
    pub fn drag(
        &mut self,
        screen_col: u16,
        screen_row: u16,
        pane_inner: Rect,
        metrics: Option<ScrollMetrics>,
    ) {
        let (viewport_row, col) = clamp_to_pane(screen_col, screen_row, pane_inner);
        self.cursor = (absolute_row_for_viewport_row(viewport_row, metrics), col);
        if self.cursor != self.anchor {
            self.phase = Phase::Dragging;
        }
    }

    /// Finalize the selection. Returns true when the user actually
    /// dragged (not just clicked). Returns false for plain clicks.
    pub fn finish(&mut self) -> bool {
        if self.phase == Phase::Dragging {
            self.phase = Phase::Done;
            true
        } else {
            false
        }
    }

    /// Whether this selection should be rendered (highlight visible).
    pub fn is_visible(&self) -> bool {
        self.phase == Phase::Dragging || self.phase == Phase::Done
    }

    /// Whether this selection was already finalized.
    #[cfg(test)]
    pub fn is_finalized(&self) -> bool {
        self.phase == Phase::Done
    }

    /// Whether the user just clicked without dragging (not a selection).
    pub fn is_just_click(&self) -> bool {
        self.phase == Phase::Anchored
    }

    /// Force the selection into Dragging phase, used when the mouse
    /// has moved off the anchor cell but drag() couldn't transition
    /// because the cursor was clamped to the same cell as the anchor.
    pub fn force_dragging(&mut self) {
        if self.phase == Phase::Anchored {
            self.phase = Phase::Dragging;
        }
    }

    /// Whether the pointer is still down and the selection can keep extending.
    pub fn is_in_progress(&self) -> bool {
        matches!(self.phase, Phase::Anchored | Phase::Dragging)
    }

    /// Whether the user is actively dragging (cursor moved from anchor).
    pub fn is_dragging(&self) -> bool {
        self.phase == Phase::Dragging
    }

    /// Returns (start, end) in reading order (top-left to bottom-right).
    fn ordered(&self) -> ((u32, u16), (u32, u16)) {
        let (ar, ac) = self.anchor;
        let (cr, cc) = self.cursor;
        if ar < cr || (ar == cr && ac <= cc) {
            ((ar, ac), (cr, cc))
        } else {
            ((cr, cc), (ar, ac))
        }
    }

    pub(crate) fn ordered_cells(&self) -> ((u32, u16), (u32, u16)) {
        self.ordered()
    }

    #[cfg(test)]
    pub(crate) fn visible_rects(&self, inner: Rect, metrics: Option<ScrollMetrics>) -> [Rect; 3] {
        let mut rects = [Rect::default(); 3];
        if !self.is_visible() || inner.is_empty() {
            return rects;
        }
        let ((start_row, start_col), (end_row, end_col)) = self.ordered();
        let top = viewport_top_row(metrics);
        let bottom = top.saturating_add(u32::from(inner.height));
        let rect = |first: u32, end: u32, left: u16, right: u16| {
            let first = first.max(top);
            let end = end.min(bottom);
            let left = left.min(inner.width);
            let right = right.min(inner.width);
            if first >= end || left >= right {
                Rect::default()
            } else {
                Rect::new(
                    inner.x + left,
                    inner.y + (first - top) as u16,
                    right - left,
                    (end - first) as u16,
                )
            }
        };
        if start_row == end_row {
            rects[0] = rect(
                start_row,
                start_row.saturating_add(1),
                start_col,
                end_col.saturating_add(1),
            );
        } else {
            rects[0] = rect(
                start_row,
                start_row.saturating_add(1),
                start_col,
                inner.width,
            );
            rects[1] = rect(start_row.saturating_add(1), end_row, 0, inner.width);
            rects[2] = rect(
                end_row,
                end_row.saturating_add(1),
                0,
                end_col.saturating_add(1),
            );
        }
        rects
    }

    /// Check whether a pane-relative cell (row, col) is inside the selection.
    pub fn contains(&self, viewport_row: u16, col: u16, metrics: Option<ScrollMetrics>) -> bool {
        if !self.is_visible() {
            return false;
        }
        let row = absolute_row_for_viewport_row(viewport_row, metrics);
        let ((sr, sc), (er, ec)) = self.ordered();
        if row < sr || row > er {
            return false;
        }
        if sr == er {
            col >= sc && col <= ec
        } else if row == sr {
            col >= sc
        } else if row == er {
            col <= ec
        } else {
            true
        }
    }
}

fn viewport_top_row(metrics: Option<ScrollMetrics>) -> u32 {
    metrics
        .map(|metrics| {
            metrics
                .max_offset_from_bottom
                .saturating_sub(metrics.offset_from_bottom)
        })
        .unwrap_or(0) as u32
}

pub(crate) fn absolute_row_for_viewport(viewport_row: u16, metrics: Option<ScrollMetrics>) -> u32 {
    absolute_row_for_viewport_row(viewport_row, metrics)
}

fn absolute_row_for_viewport_row(viewport_row: u16, metrics: Option<ScrollMetrics>) -> u32 {
    viewport_top_row(metrics) + u32::from(viewport_row)
}

fn viewport_row_for_absolute_row(absolute_row: u32, metrics: Option<ScrollMetrics>) -> u16 {
    absolute_row
        .saturating_sub(viewport_top_row(metrics))
        .try_into()
        .unwrap_or(0)
}

fn clamp_to_pane(screen_col: u16, screen_row: u16, pane_inner: Rect) -> (u16, u16) {
    let clamped_col = screen_col.clamp(
        pane_inner.x,
        pane_inner.x + pane_inner.width.saturating_sub(1),
    );
    let clamped_row = screen_row.clamp(
        pane_inner.y,
        pane_inner.y + pane_inner.height.saturating_sub(1),
    );
    (clamped_row - pane_inner.y, clamped_col - pane_inner.x)
}

/// OSC 52 format: `ESC ] 52 ; c ; <base64> BEL`.
///
/// Some terminals still only honor BEL-terminated OSC 52 writes, so herdr
/// emits BEL here even though ST works in newer emulators.
pub(crate) fn osc52_sequence(bytes: &[u8]) -> String {
    format!("\x1b]52;c;{}\x07", base64_encode(bytes))
}

/// Standard-alphabet base64 with padding — the `base64` crate's
/// `engine::general_purpose::STANDARD` output, kept local so the crate
/// carries no extra dependency for one short encoding.
pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
        let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(n >> 18) as usize & 0x3f] as char);
        out.push(ALPHABET[(n >> 12) as usize & 0x3f] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 0x3f] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 0x3f] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_rects_match_selection_cells_when_scrolled_and_reversed() {
        let inner = Rect::new(10, 5, 8, 6);
        for (start, end) in [
            ((0, 2), (0, 5)),
            ((1, 3), (4, 2)),
            ((4, 2), (1, 3)),
            ((0, 0), (20, 7)),
            ((20, 0), (22, 7)),
        ] {
            let selection = Selection::absolute_range((), start, end);
            for top in [0, 2, 5, 20] {
                let metrics = Some(ScrollMetrics {
                    offset_from_bottom: 0,
                    max_offset_from_bottom: top,
                    viewport_rows: 6,
                });
                let rects = selection.visible_rects(inner, metrics);
                for row in 0..inner.height {
                    for col in 0..inner.width {
                        assert_eq!(
                            rects
                                .iter()
                                .any(|rect| rect.contains((inner.x + col, inner.y + row).into())),
                            selection.contains(row, col, metrics),
                            "start={start:?} end={end:?} top={top} row={row} col={col}",
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn osc52_sequence_uses_bel_terminator() {
        assert_eq!(osc52_sequence(b"hello"), "\x1b]52;c;aGVsbG8=\x07");
        assert_eq!(osc52_sequence(b"he"), "\x1b]52;c;aGU=\x07");
        assert_eq!(osc52_sequence(b"h"), "\x1b]52;c;aA==\x07");
        assert_eq!(osc52_sequence(b""), "\x1b]52;c;\x07");
    }

    #[test]
    fn click_then_drag_transitions_phases() {
        let mut sel = Selection::anchor("p".to_owned(), 0, 0, None);
        assert!(sel.is_just_click());
        assert!(!sel.is_visible());
        sel.drag(5, 0, Rect::new(0, 0, 10, 5), None);
        assert!(sel.is_visible());
        assert!(sel.is_in_progress());
        assert!(sel.finish());
        assert!(sel.is_finalized());
        assert!(!sel.is_in_progress());
        assert!(sel.is_visible());
    }

    #[test]
    fn absolute_rows_keep_selection_stable_across_scroll() {
        // Anchor viewport row 0 while scrolled: absolute row = top + 0.
        let metrics = Some(ScrollMetrics {
            offset_from_bottom: 3,
            max_offset_from_bottom: 5,
            viewport_rows: 4,
        });
        let mut sel = Selection::anchor("p".to_owned(), 0, 1, metrics);
        sel.drag(3, 0, Rect::new(0, 0, 10, 4), metrics);
        assert_eq!(sel.ordered_cells(), ((2, 1), (2, 3)));
        // Scrolling to the bottom re-maps the same absolute rows to
        // viewport rows above the visible area: nothing is selected.
        let bottom = Some(ScrollMetrics {
            offset_from_bottom: 0,
            max_offset_from_bottom: 5,
            viewport_rows: 4,
        });
        for row in 0..4 {
            for col in 0..10 {
                assert!(!sel.contains(row, col, bottom));
            }
        }
    }
}

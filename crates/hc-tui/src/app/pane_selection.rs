// Derived from herdr src/client/shell/mouse.rs, src/client/shell/state.rs,
// src/client/shell/word_selection.rs and src/client/shell/actions.rs
// (Apache-2.0).
//! Pane text selection gestures. A non-reporting left press anchors a
//! `Selection` on absolute screen rows; drags clamp to the pane's inner
//! rect and autoscroll through history past the top/bottom edge at the
//! reference's 30 ms interval (one row per tick; 3–15 rows for distance
//! past the edge). Release extracts text from the pane's own displayed VT
//! — no core call — and writes OSC 52 through `host_control` when
//! `copy_on_select` is on; otherwise the highlight stays until Ctrl+C or
//! the next click/key clears it.
//!
//! Double-click starts the word gesture: the reference resolves rows via
//! a `PaneSelectionRead` RPC, hc-tui reads the same absolute rows from the
//! local VT synchronously, so there is no generation/pending bookkeeping.
//!
//! The reference's 16 ms `selection_repaint_deadline` has no counterpart:
//! `sched::Scheduler` already coalesces every dirty mark into the ~60 FPS
//! frame slot, which is the same throttle.

use std::time::{Duration, Instant};

use ratatui::layout::Rect;

use super::App;
use super::input::{MouseEventKind, MouseInput};
use crate::chrome::render::PaneHit;
use crate::chrome::scroll::ScrollMetrics;
use crate::selection::Selection;

/// The reference's `SELECTION_AUTOSCROLL_INTERVAL`.
const SELECTION_AUTOSCROLL_INTERVAL: Duration = Duration::from_millis(30);
/// The reference's 350 ms double-click window.
const DOUBLE_CLICK: Duration = Duration::from_millis(350);

/// The reference's `ClientPaneClick` — double-click bookkeeping.
#[derive(Clone, Debug)]
pub(super) struct PaneClick {
    pub pane_id: String,
    pub viewport_row: u16,
    pub col: u16,
    pub at: Instant,
}

impl PaneClick {
    fn is_double_click_for(&self, next: &Self) -> bool {
        self.pane_id == next.pane_id
            && next.at.duration_since(self.at) <= DOUBLE_CLICK
            && self.viewport_row.abs_diff(next.viewport_row) <= 1
            && self.col.abs_diff(next.col) <= 1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AutoscrollDirection {
    Up,
    Down,
}

/// The reference's `ClientSelectionAutoscroll`.
#[derive(Clone, Debug)]
pub(super) struct SelectionAutoscroll {
    pub pane_id: String,
    pub direction: AutoscrollDirection,
    pub last_mouse_column: u16,
    pub last_mouse_row: u16,
    pub inner_rect: Rect,
    pub offset_from_bottom: usize,
    pub max_offset_from_bottom: usize,
}

impl App {
    /// Down(Left) prologue — every left press reaching the main dispatch
    /// retires the selection family first and returns the previous pane
    /// click for double-click matching (the reference's `Down(Left)` arm
    /// prologue).
    pub(super) fn clear_selection_for_press(&mut self) -> Option<PaneClick> {
        if self.selection.take().is_some() {
            self.sched.mark_dirty(Instant::now());
        }
        self.stop_selection_autoscroll();
        self.selection_highlight_clear_deadline = None;
        self.word_selection = None;
        self.last_pane_click.take()
    }

    /// Non-reporting left press inside the pane's inner rect — anchor a
    /// selection, or start the word gesture when this press double-clicks
    /// the previous one (the reference's `!hit.mouse_reporting` branch).
    pub(super) fn press_pane_selection(
        &mut self,
        hit: &PaneHit,
        mouse: &MouseInput,
        previous: Option<PaneClick>,
    ) {
        let click = PaneClick {
            pane_id: hit.pane_id.clone(),
            viewport_row: mouse.row.saturating_sub(hit.inner_rect.y),
            col: mouse.col.saturating_sub(hit.inner_rect.x),
            at: Instant::now(),
        };
        if mouse.mods.is_empty()
            && previous
                .as_ref()
                .is_some_and(|previous| previous.is_double_click_for(&click))
        {
            self.request_word_selection(hit, click.viewport_row, click.col);
        } else {
            if mouse.mods.is_empty() {
                self.last_pane_click = Some(click.clone());
            }
            self.selection = Some(Selection::anchor(
                hit.pane_id.clone(),
                click.viewport_row,
                click.col,
                hit.scroll,
            ));
        }
    }

    fn selection_edge_scroll_lines(distance: u16) -> usize {
        usize::from(distance).saturating_mul(3).clamp(3, 15)
    }

    fn selection_scroll_metrics(&self, hit: &PaneHit) -> Option<ScrollMetrics> {
        let metrics = hit.scroll?;
        Some(
            self.selection_autoscroll
                .as_ref()
                .filter(|autoscroll| autoscroll.pane_id == hit.pane_id)
                .map_or(metrics, |autoscroll| ScrollMetrics {
                    offset_from_bottom: autoscroll.offset_from_bottom,
                    max_offset_from_bottom: autoscroll.max_offset_from_bottom,
                    viewport_rows: metrics.viewport_rows,
                }),
        )
    }

    /// The pane whose in-progress selection owns the pointer — by hit
    /// identity, not pointer position, so drags outside the inner rect
    /// keep extending.
    pub(super) fn active_selection_pane(&self) -> Option<PaneHit> {
        let pane_id = if let Some(gesture) = self.word_selection.as_ref() {
            if gesture.released {
                return None;
            }
            &gesture.pane_id
        } else {
            &self
                .selection
                .as_ref()
                .filter(|selection| selection.is_in_progress())?
                .pane_id
        };
        self.hit_map
            .panes
            .iter()
            .find(|hit| hit.pane_id == *pane_id)
            .cloned()
    }

    fn update_selection_cursor_with_metrics(
        &mut self,
        hit: &PaneHit,
        column: u16,
        row: u16,
        metrics: Option<ScrollMetrics>,
    ) {
        if self.word_selection.is_some() {
            let viewport_row = row
                .saturating_sub(hit.inner_rect.y)
                .min(hit.inner_rect.height.saturating_sub(1));
            let col = column
                .saturating_sub(hit.inner_rect.x)
                .min(hit.inner_rect.width.saturating_sub(1));
            let absolute_row = crate::selection::absolute_row_for_viewport(viewport_row, metrics);
            self.drag_word_selection((absolute_row, col));
        } else if let Some(selection) = self.selection.as_mut() {
            selection.drag(column, row, hit.inner_rect, metrics);
        }
    }

    /// A `Drag(Left)` while a selection is in progress — clamped cursor
    /// extension plus the reference's edge autoscroll arming.
    pub(super) fn update_selection_drag(&mut self, hit: &PaneHit, column: u16, row: u16) {
        let metrics = self.selection_scroll_metrics(hit);
        let was_dragging = self.selection.as_ref().is_some_and(Selection::is_dragging);
        let moved_from_anchor = self.selection.as_ref().is_some_and(|selection| {
            let (anchor_row, anchor_col) = selection.anchor_screen_pos(hit.inner_rect, metrics);
            anchor_row != row || anchor_col != column
        });
        self.update_selection_cursor_with_metrics(hit, column, row, metrics);
        let is_dragging = self
            .word_selection
            .as_ref()
            .map_or(was_dragging || moved_from_anchor, |gesture| gesture.dragged);
        if is_dragging {
            if let Some(selection) = self.selection.as_mut()
                && selection.is_just_click()
            {
                selection.force_dragging();
            }
            self.last_pane_click = None;
        }
        if !is_dragging {
            self.stop_selection_autoscroll();
            return;
        }

        let Some(metrics) = metrics else {
            self.stop_selection_autoscroll();
            return;
        };
        let top = hit.inner_rect.y;
        let bottom = hit.inner_rect.y + hit.inner_rect.height.saturating_sub(1);
        let (direction, immediate_lines) = if row < top {
            (
                AutoscrollDirection::Up,
                Self::selection_edge_scroll_lines(top - row),
            )
        } else if row > bottom {
            (
                AutoscrollDirection::Down,
                Self::selection_edge_scroll_lines(row - bottom),
            )
        } else if row == top {
            (AutoscrollDirection::Up, 0)
        } else if row == bottom {
            (AutoscrollDirection::Down, 0)
        } else {
            self.stop_selection_autoscroll();
            return;
        };

        let offset_from_bottom = match direction {
            AutoscrollDirection::Up => metrics
                .offset_from_bottom
                .saturating_add(immediate_lines)
                .min(metrics.max_offset_from_bottom),
            AutoscrollDirection::Down => metrics.offset_from_bottom.saturating_sub(immediate_lines),
        };
        if offset_from_bottom != metrics.offset_from_bottom {
            let projected = ScrollMetrics {
                offset_from_bottom,
                ..metrics
            };
            self.update_selection_cursor_with_metrics(hit, column, row, Some(projected));
            self.pane_scroll_to_offset(&hit.pane_id, offset_from_bottom);
        }
        self.selection_autoscroll = Some(SelectionAutoscroll {
            pane_id: hit.pane_id.clone(),
            direction,
            last_mouse_column: column,
            last_mouse_row: row,
            inner_rect: hit.inner_rect,
            offset_from_bottom,
            max_offset_from_bottom: metrics.max_offset_from_bottom,
        });
        self.selection_autoscroll_deadline = Some(Instant::now() + SELECTION_AUTOSCROLL_INTERVAL);
    }

    /// A wheel event while a selection is in progress scrolls the
    /// selection's own pane and extends the cursor with the projected
    /// metrics — the wheel never reaches chrome or program routing.
    pub(super) fn scroll_in_progress_selection(&mut self, mouse: MouseInput) -> bool {
        if !matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            return false;
        }
        let Some(hit) = self.active_selection_pane() else {
            return false;
        };
        let Some(metrics) = self.selection_scroll_metrics(&hit) else {
            return false;
        };
        let offset_from_bottom = match mouse.kind {
            MouseEventKind::ScrollUp => metrics
                .offset_from_bottom
                .saturating_add(self.shell.mouse_scroll_lines)
                .min(metrics.max_offset_from_bottom),
            MouseEventKind::ScrollDown => metrics
                .offset_from_bottom
                .saturating_sub(self.shell.mouse_scroll_lines),
            _ => unreachable!(),
        };
        if offset_from_bottom != metrics.offset_from_bottom {
            let projected = ScrollMetrics {
                offset_from_bottom,
                ..metrics
            };
            self.update_selection_cursor_with_metrics(&hit, mouse.col, mouse.row, Some(projected));
            self.pane_scroll_to_offset(&hit.pane_id, offset_from_bottom);
        }
        true
    }

    /// The 30 ms edge-autoscroll tick plus the 500 ms word-highlight
    /// clear. Returns true when the frame must repaint.
    pub(super) fn tick_selection(&mut self, now: Instant) -> bool {
        let mut repaint = false;
        if self
            .selection_highlight_clear_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            self.selection = None;
            self.selection_highlight_clear_deadline = None;
            repaint = true;
        }
        if self
            .selection_autoscroll_deadline
            .is_none_or(|deadline| now < deadline)
        {
            return repaint;
        }
        let Some(mut autoscroll) = self.selection_autoscroll.clone() else {
            self.selection_autoscroll_deadline = None;
            return repaint;
        };
        let dragging = self.word_selection.as_ref().map_or_else(
            || {
                self.selection.as_ref().is_some_and(|selection| {
                    selection.pane_id == autoscroll.pane_id && selection.is_dragging()
                })
            },
            |gesture| gesture.pane_id == autoscroll.pane_id && gesture.dragged && !gesture.released,
        );
        if !dragging {
            self.stop_selection_autoscroll();
            return repaint;
        }
        let Some(hit) = self
            .hit_map
            .panes
            .iter()
            .find(|hit| hit.pane_id == autoscroll.pane_id)
            .cloned()
        else {
            self.stop_selection_autoscroll();
            return repaint;
        };
        if hit.inner_rect != autoscroll.inner_rect {
            self.stop_selection_autoscroll();
            return repaint;
        }
        let next_offset = match autoscroll.direction {
            AutoscrollDirection::Up => autoscroll
                .offset_from_bottom
                .saturating_add(1)
                .min(autoscroll.max_offset_from_bottom),
            AutoscrollDirection::Down => autoscroll.offset_from_bottom.saturating_sub(1),
        };
        if next_offset == autoscroll.offset_from_bottom {
            self.stop_selection_autoscroll();
            return repaint;
        }
        autoscroll.offset_from_bottom = next_offset;
        let metrics = ScrollMetrics {
            offset_from_bottom: next_offset,
            max_offset_from_bottom: autoscroll.max_offset_from_bottom,
            viewport_rows: hit.scroll.map_or(0, |metrics| metrics.viewport_rows),
        };
        self.update_selection_cursor_with_metrics(
            &hit,
            autoscroll.last_mouse_column,
            autoscroll.last_mouse_row,
            Some(metrics),
        );
        self.pane_scroll_to_offset(&autoscroll.pane_id, next_offset);
        self.selection_autoscroll = Some(autoscroll);
        self.selection_autoscroll_deadline = Some(now + SELECTION_AUTOSCROLL_INTERVAL);
        true
    }

    pub(super) fn stop_selection_autoscroll(&mut self) {
        self.selection_autoscroll = None;
        self.selection_autoscroll_deadline = None;
    }

    /// `Up(Left)` while a selection exists — a real drag copies under
    /// `copy_on_select` or stays highlighted otherwise; a plain click
    /// selects nothing and copies nothing.
    pub(super) fn finish_selection(&mut self) {
        self.stop_selection_autoscroll();
        let copied = self.selection.as_mut().is_some_and(Selection::finish);
        if copied && self.shell.copy_on_select {
            self.request_selection_copy();
            self.selection = None;
        } else if self
            .selection
            .as_ref()
            .is_some_and(Selection::is_just_click)
        {
            self.selection = None;
        }
        if copied {
            self.last_pane_click = None;
        }
    }
}

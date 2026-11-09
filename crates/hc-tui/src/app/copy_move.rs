// Derived from herdr src/client/shell/copy_mode.rs (Apache-2.0).
//! Copy-mode cursor movement and selection sync: arrow/page/history
//! motions, cursor reveal, and rebuilding the visible selection from
//! the copy cursor. Entry/exit and focus/surface sync live in
//! `app/copy_mode.rs`; search and key routing in `app/copy_keys.rs`.

use std::time::Instant;

use super::App;
use super::copy_mode::CopySelection;

impl App {
    /// The reference's `move_copy_cursor`: arrow-key steps in absolute
    /// screen-buffer coordinates, clamped to the stored geometry.
    pub(super) fn move_copy_cursor(&mut self, row_delta: i32, col_delta: i32) {
        let Some(copy_mode) = self.copy_mode.as_mut() else {
            return;
        };
        let (width, height) = copy_mode.geometry;
        if col_delta < 0 {
            copy_mode.cursor.col = copy_mode
                .cursor
                .col
                .saturating_sub(col_delta.unsigned_abs() as u16);
        } else if col_delta > 0 {
            copy_mode.cursor.col = copy_mode
                .cursor
                .col
                .saturating_add(col_delta as u16)
                .min(width.saturating_sub(1));
        }
        let total_rows = copy_mode
            .max_offset_from_bottom
            .saturating_add(usize::from(height))
            .max(1);
        if row_delta < 0 {
            copy_mode.cursor.row = copy_mode
                .cursor
                .row
                .saturating_sub(row_delta.unsigned_abs());
        } else if row_delta > 0 {
            copy_mode.cursor.row = copy_mode
                .cursor
                .row
                .saturating_add(row_delta as u32)
                .min(total_rows.saturating_sub(1).min(u32::MAX as usize) as u32);
        }
        self.reveal_copy_cursor(false);
        self.sync_copy_selection();
        self.sched.mark_dirty(Instant::now());
    }

    /// The reference's `move_copy_page`: moves the cursor and the scroll
    /// offset together by a pane height (half for ctrl+u/ctrl+d).
    pub(super) fn move_copy_page(&mut self, direction: i32, half_page: bool) {
        let Some(hit) = self.copy_hit() else {
            return;
        };
        let Some(copy_mode) = self.copy_mode.as_mut() else {
            return;
        };
        let step = u16::try_from(crate::copy_mode::copy_mode_page_lines(
            hit.inner_rect.height,
            half_page,
        ))
        .unwrap_or(u16::MAX)
        .max(1);
        if direction < 0 {
            copy_mode.cursor.row = copy_mode.cursor.row.saturating_sub(u32::from(step));
            copy_mode.offset_from_bottom = (copy_mode
                .offset_from_bottom
                .saturating_add(usize::from(step)))
            .min(copy_mode.max_offset_from_bottom);
        } else {
            let last_row = copy_mode
                .max_offset_from_bottom
                .saturating_add(usize::from(hit.inner_rect.height))
                .saturating_sub(1)
                .min(u32::MAX as usize) as u32;
            copy_mode.cursor.row = copy_mode
                .cursor
                .row
                .saturating_add(u32::from(step))
                .min(last_row);
            copy_mode.offset_from_bottom = copy_mode
                .offset_from_bottom
                .saturating_sub(usize::from(step));
        }
        let pane_id = copy_mode.pane_id.clone();
        let offset = copy_mode.offset_from_bottom;
        self.pane_scroll_to_offset(&pane_id, offset);
        self.sync_copy_selection();
        self.sched.mark_dirty(Instant::now());
    }

    /// The reference's `move_copy_history`: `g`/`G` jump to the oldest
    /// or newest row and pin the scroll offset accordingly.
    pub(super) fn move_copy_history(&mut self, top: bool) {
        let Some(hit) = self.copy_hit() else {
            return;
        };
        let Some(copy_mode) = self.copy_mode.as_mut() else {
            return;
        };
        let last_row = u32::try_from(copy_mode.max_offset_from_bottom)
            .unwrap_or(u32::MAX)
            .saturating_add(u32::from(hit.inner_rect.height.saturating_sub(1)));
        copy_mode.cursor.row = if top { 0 } else { last_row };
        copy_mode.offset_from_bottom = if top {
            copy_mode.max_offset_from_bottom
        } else {
            0
        };
        let pane_id = copy_mode.pane_id.clone();
        let offset = copy_mode.offset_from_bottom;
        self.pane_scroll_to_offset(&pane_id, offset);
        self.sync_copy_selection();
        self.sched.mark_dirty(Instant::now());
    }

    /// The reference's `set_copy_cursor_col` — `0` and `$`.
    pub(super) fn set_copy_cursor_col(&mut self, col: u16) {
        let Some(copy_mode) = self.copy_mode.as_mut() else {
            return;
        };
        copy_mode.cursor.col = col;
        self.reveal_copy_cursor(false);
        self.sync_copy_selection();
        self.sched.mark_dirty(Instant::now());
    }

    /// The reference's `reveal_copy_cursor`: scroll just enough for the
    /// copy cursor to be visible. `reserve_mode_bar_row` (used when a
    /// search lands) keeps one extra row clear for the mode bar.
    pub(super) fn reveal_copy_cursor(&mut self, reserve_mode_bar_row: bool) {
        let request = self.copy_mode.as_mut().and_then(|copy_mode| {
            let current_top = copy_mode
                .max_offset_from_bottom
                .saturating_sub(copy_mode.offset_from_bottom) as u32;
            let max_cursor_row = copy_mode
                .geometry
                .1
                .saturating_sub(if reserve_mode_bar_row { 2 } else { 1 });
            let bottom = current_top.saturating_add(u32::from(max_cursor_row));
            let desired_top = if copy_mode.cursor.row < current_top {
                copy_mode.cursor.row
            } else if copy_mode.cursor.row > bottom {
                copy_mode
                    .cursor
                    .row
                    .saturating_sub(u32::from(max_cursor_row))
            } else {
                current_top
            };
            let offset = copy_mode
                .max_offset_from_bottom
                .saturating_sub(desired_top as usize);
            if offset == copy_mode.offset_from_bottom {
                return None;
            }
            copy_mode.offset_from_bottom = offset;
            Some((copy_mode.pane_id.clone(), offset))
        });
        if let Some((pane_id, offset)) = request {
            self.pane_scroll_to_offset(&pane_id, offset);
            self.sched.mark_dirty(Instant::now());
        }
    }

    /// The reference's `begin_copy_selection`: `v` anchors a character
    /// selection at the cursor, `V` anchors a linewise one. Pressing a
    /// selection key again re-anchors at the current cursor.
    pub(super) fn begin_copy_selection(&mut self, linewise: bool) {
        let width = self.copy_hit().map(|hit| hit.inner_rect.width);
        let Some(copy_mode) = self.copy_mode.as_mut() else {
            return;
        };
        let end_col = width.unwrap_or(copy_mode.geometry.0).saturating_sub(1);
        if linewise {
            copy_mode.selection = Some(CopySelection::Linewise {
                anchor_row: copy_mode.cursor.row,
            });
            self.selection = Some(crate::selection::Selection::line_range(
                copy_mode.pane_id.clone(),
                copy_mode.cursor.row,
                copy_mode.cursor.row,
                end_col,
            ));
        } else {
            copy_mode.selection = Some(CopySelection::Character {
                anchor: copy_mode.cursor,
            });
            self.selection = Some(crate::selection::Selection::absolute_anchor(
                copy_mode.pane_id.clone(),
                (copy_mode.cursor.row, copy_mode.cursor.col),
            ));
        }
    }

    /// The reference's `sync_copy_selection`: rebuilds the visible
    /// selection from the copy selection and the cursor's new cell.
    pub(super) fn sync_copy_selection(&mut self) {
        let Some(copy_mode) = self.copy_mode.as_ref() else {
            return;
        };
        let Some(selection) = copy_mode.selection else {
            return;
        };
        self.selection = Some(match selection {
            CopySelection::Character { anchor } => crate::selection::Selection::absolute_range(
                copy_mode.pane_id.clone(),
                (anchor.row, anchor.col),
                (copy_mode.cursor.row, copy_mode.cursor.col),
            ),
            CopySelection::Linewise { anchor_row } => crate::selection::Selection::line_range(
                copy_mode.pane_id.clone(),
                anchor_row,
                copy_mode.cursor.row,
                self.copy_hit()
                    .map_or(copy_mode.geometry.0, |hit| hit.inner_rect.width)
                    .saturating_sub(1),
            ),
        });
    }
}

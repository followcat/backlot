// Derived from herdr src/pane/terminal.rs `Pane::word_motion_target`,
// `Pane::paragraph_motion_target` and `Pane::search_text_window`
// (Apache-2.0).
//! Vt-level copy scan entry points: the sliding-window word motion, the
//! paragraph scan and the retained-buffer search window the app calls.

use hc_vt::Vt;

use crate::copy_buffer::{
    CopySearchWindow, PARAGRAPH_SCAN_LIMIT, RetainedTextBuffer, WORD_MOTION_WINDOW, WordMotion,
    terminal_cell_text,
};
use crate::copy_mode::{CopySearchDirection, PaneTextPoint, PaneTextRange};

/// The reference's `Pane::word_motion_target`: a 64-row sliding window
/// that doubles until the answer is stable or the buffer edge is hit.
pub(crate) fn word_motion_target(
    vt: &Vt,
    row: u32,
    col: u16,
    motion: WordMotion,
) -> Option<PaneTextPoint> {
    let total_rows = vt.total_rows().ok()?;
    let row = usize::try_from(row).ok()?;
    if row >= total_rows {
        return None;
    }

    let mut window_rows = WORD_MOTION_WINDOW;
    loop {
        let (start_row, end_row) = match motion {
            WordMotion::PreviousStart | WordMotion::PreviousBigStart => {
                (row.saturating_sub(window_rows.saturating_sub(1)), row + 1)
            }
            WordMotion::NextStart
            | WordMotion::NextEnd
            | WordMotion::NextBigStart
            | WordMotion::NextBigEnd => (row, row.saturating_add(window_rows).min(total_rows)),
        };
        let rows = vt.screen_text_rows_range(start_row, end_row).ok()?;
        let starts_in_continuation = rows
            .first()
            .is_some_and(|row| row.wrap_continuation && start_row > 0);
        let ends_in_continuation = rows
            .last()
            .is_some_and(|row| row.soft_wrapped && end_row < total_rows);
        let buffer = RetainedTextBuffer::new_words(rows, u32::try_from(start_row).ok()?);
        let target = buffer.word_motion(u32::try_from(row).ok()?, col, motion);
        let needs_more_history = matches!(
            motion,
            WordMotion::PreviousStart | WordMotion::PreviousBigStart
        ) && target
            .is_some_and(|target| starts_in_continuation && target.row == start_row as u32);
        let needs_more_future = matches!(motion, WordMotion::NextEnd | WordMotion::NextBigEnd)
            && ends_in_continuation
            && target.is_some_and(|target| buffer.point_is_final_atom(target));
        if target.is_some() && !needs_more_history && !needs_more_future {
            return target;
        }

        let reached_edge = match motion {
            WordMotion::PreviousStart | WordMotion::PreviousBigStart => start_row == 0,
            WordMotion::NextStart
            | WordMotion::NextEnd
            | WordMotion::NextBigStart
            | WordMotion::NextBigEnd => end_row == total_rows,
        };
        if reached_edge {
            return target;
        }
        window_rows = window_rows.saturating_mul(2).min(total_rows);
    }
}

/// The reference's `Pane::paragraph_motion_target`: the first
/// all-whitespace row within 1000 rows, keeping the cursor's column.
pub(crate) fn paragraph_motion_target(vt: &Vt, row: u32, direction: i8) -> Option<PaneTextPoint> {
    let total_rows = vt.total_rows().ok()?;
    let current = usize::try_from(row).ok()?;
    if current >= total_rows || direction == 0 {
        return None;
    }
    let limit = total_rows.min(PARAGRAPH_SCAN_LIMIT);
    for distance in 1..limit {
        let candidate = if direction < 0 {
            current.checked_sub(distance)?
        } else {
            let candidate = current.saturating_add(distance);
            if candidate >= total_rows {
                return None;
            }
            candidate
        };
        let rows = vt
            .screen_text_rows_range(candidate, candidate.saturating_add(1))
            .ok()?;
        let row = rows.first()?;
        let is_blank = row.cells.iter().all(|cell| {
            terminal_cell_text(&cell.graphemes)
                .chars()
                .all(char::is_whitespace)
        });
        if is_blank {
            return Some(PaneTextPoint {
                row: u32::try_from(candidate).ok()?,
                col: 0,
            });
        }
    }
    None
}

/// The reference's `Pane::search_text_window`: one pass over the whole
/// retained buffer (scrollback plus viewport).
pub(crate) fn search_text_window(
    vt: &Vt,
    query: &str,
    case_sensitive: bool,
    direction: CopySearchDirection,
    cursor: PaneTextPoint,
    previous: Option<PaneTextRange>,
    limit: usize,
) -> CopySearchWindow {
    let Ok(rows) = vt.screen_text_rows_range(0, usize::MAX) else {
        return CopySearchWindow::default();
    };
    RetainedTextBuffer::new_search(rows, 0).search_window(
        query,
        case_sensitive,
        direction,
        cursor,
        previous,
        limit,
    )
}

#[cfg(test)]
mod tests {
    use hc_vt::{ScreenTextRow, Wide};

    use super::*;

    fn text_row(text: &str, soft_wrapped: bool) -> ScreenTextRow {
        ScreenTextRow {
            cells: text
                .chars()
                .map(|c| hc_vt::ScreenTextCell {
                    wide: Wide::Narrow,
                    graphemes: vec![c as u32],
                })
                .collect(),
            soft_wrapped,
            wrap_continuation: false,
        }
    }

    #[test]
    fn search_is_literal_and_case_aware() {
        let buffer = RetainedTextBuffer::new(vec![text_row("hello HELLO", false)]);
        let cursor = PaneTextPoint { row: 0, col: 0 };
        let sensitive = buffer.search_window(
            "hello",
            true,
            CopySearchDirection::Forward,
            cursor,
            None,
            16,
        );
        assert_eq!(sensitive.total, 1);
        let insensitive = buffer.search_window(
            "hello",
            false,
            CopySearchDirection::Forward,
            cursor,
            None,
            16,
        );
        assert_eq!(insensitive.total, 2);
    }

    #[test]
    fn search_crosses_soft_wraps_but_not_hard_lines() {
        let buffer =
            RetainedTextBuffer::new(vec![text_row("abcde", true), text_row("fg xy", false)]);
        let cursor = PaneTextPoint { row: 0, col: 0 };
        let window =
            buffer.search_window("def", true, CopySearchDirection::Forward, cursor, None, 16);
        assert_eq!(window.total, 1);
        // The end point is the inclusive last cell of the final grapheme —
        // 'f' sits at row 1 col 0 after the soft wrap.
        assert_eq!(
            window.matches[0],
            PaneTextRange {
                start: PaneTextPoint { row: 0, col: 3 },
                end: PaneTextPoint { row: 1, col: 0 },
            }
        );
    }

    #[test]
    fn word_motions_use_tmux_separators() {
        let buffer = RetainedTextBuffer::new(vec![text_row("foo-bar baz", false)]);
        let start = buffer
            .word_motion(0, 0, WordMotion::NextStart)
            .expect("next start");
        assert_eq!(start, PaneTextPoint { row: 0, col: 3 });
        let end = buffer
            .word_motion(0, 0, WordMotion::NextEnd)
            .expect("next end");
        assert_eq!(end, PaneTextPoint { row: 0, col: 2 });
    }

    #[test]
    fn big_word_motions_treat_only_whitespace_as_separators() {
        let buffer = RetainedTextBuffer::new(vec![text_row("foo-bar baz", false)]);
        let start = buffer
            .word_motion(0, 0, WordMotion::NextBigStart)
            .expect("next big start");
        assert_eq!(start, PaneTextPoint { row: 0, col: 8 });
        let end = buffer
            .word_motion(0, 0, WordMotion::NextBigEnd)
            .expect("next big end");
        assert_eq!(end, PaneTextPoint { row: 0, col: 6 });
    }
}

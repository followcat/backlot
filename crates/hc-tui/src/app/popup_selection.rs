// Derived from herdr src/client/shell/popup_selection.rs (Apache-2.0).
//! Selection inside the floating popup terminal. Popup surfaces expose
//! visible cells rather than pane history; the selection stays in the
//! client and copies those cells — no server or wire-format change,
//! same as the reference. Copied text goes through the same OSC 52
//! host-control sink as pane selection.

use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

use crate::chrome::render::PaneHit;
use crate::selection::Selection;

use super::App;
use super::input::{MouseButton, MouseEventKind, MouseInput};
use super::mouse::contains;

/// The reference's `cell_span`: walk a visible row left to right by
/// cell widths and return the `(left, right)` columns of the cell that
/// owns `column`. Wide graphemes report their full span so a selection
/// edge never splits them. `frame_width` is the cell row stride (the
/// terminal's columns); `width` is the clipped visible bound — they
/// differ when the popup surface is narrower than its terminal.
fn cell_span(cells: &[String], frame_width: u16, width: u16, row: u16, column: u16) -> (u16, u16) {
    let mut left = 0;
    while left < width {
        let Some(symbol) =
            cells.get(usize::from(row) * usize::from(frame_width) + usize::from(left))
        else {
            break;
        };
        let cell_width = u16::try_from(UnicodeWidthStr::width(symbol.as_str()))
            .unwrap_or(u16::MAX)
            .max(1);
        let right = left.saturating_add(cell_width).min(width);
        if column < right {
            return (left, right - 1);
        }
        left = right;
    }
    (column, column)
}

/// The popup's visible frame flattened like the reference's
/// `FrameData.cells`: row-major `width × height` symbols, empty for
/// blanks and wide-cell tails.
fn popup_frame_cells(popup: &mut super::popup::PopupState) -> Option<(u16, u16, Vec<String>)> {
    let vt = popup.pane.vt()?;
    let (width, height) = vt.size().ok()?;
    let frame = vt.frame(0).ok()?;
    let mut cells = vec![String::new(); usize::from(width) * usize::from(height)];
    for row in &frame.rows {
        if row.y < 0 || row.y >= i32::from(height) {
            continue;
        }
        let base = row.y as usize * usize::from(width);
        for (col, cell) in row.cells.iter().enumerate() {
            if col >= usize::from(width) {
                break;
            }
            cells[base + col] = cell.text.clone();
        }
    }
    Some((width, height, cells))
}

/// The reference's `visible_selection`: clip the absolute selection to
/// the popup's visible frame and expand both edges to whole grapheme
/// spans.
fn visible_selection(
    selection: &Selection<String>,
    frame_width: u16,
    height: u16,
    cells: &[String],
    inner: Rect,
) -> Option<Selection<()>> {
    let width = frame_width.min(inner.width);
    let height = height.min(inner.height);
    if !selection.is_visible() || width == 0 || height == 0 {
        return None;
    }
    let ((mut first_row, mut first_col), (last_row, last_col)) = selection.ordered_cells();
    let clipped_last_row = last_row.min(u32::from(height - 1));
    if first_col >= width {
        first_row = first_row.saturating_add(1);
        first_col = 0;
    }
    if first_row > clipped_last_row {
        return None;
    }
    let last_col = if clipped_last_row < last_row {
        width - 1
    } else {
        last_col.min(width - 1)
    };
    let first_col = cell_span(cells, frame_width, width, first_row as u16, first_col).0;
    let last_col = cell_span(cells, frame_width, width, clipped_last_row as u16, last_col).1;
    Some(Selection::absolute_range(
        (),
        (first_row, first_col),
        (clipped_last_row, last_col),
    ))
}

/// Whether the popup terminal reports mouse input — the reference's
/// `hit.mouse_reporting`, read live since the popup hit is rebuilt each
/// frame before the flag matters.
fn popup_mouse_reporting(app: &App, hit: &PaneHit) -> bool {
    app.popup
        .as_ref()
        .filter(|popup| popup.terminal == hit.pane_id)
        .and_then(|popup| popup.pane.vt_ref())
        .is_some_and(|vt| vt.mouse_tracking())
}

/// The reference's extraction loop: emit the clipped cells row by row —
/// wide heads emit their grapheme once, blanks emit a space, and a line
/// reaching the right edge loses trailing spaces. Rows are addressed by
/// `frame_width` (the terminal's stride); `width` is only the clip bound
/// for the edge trim.
fn selection_text(
    cells: &[String],
    frame_width: u16,
    width: u16,
    clipped: &Selection<()>,
) -> String {
    let ((first_row, first_col), (last_row, last_col)) = clipped.ordered_cells();
    let mut text = String::new();
    for row in first_row..=last_row {
        if row != first_row {
            text.push('\n');
        }
        let left = if row == first_row { first_col } else { 0 };
        let right = if row == last_row { last_col } else { width - 1 };
        let start = text.len();
        let mut column = 0;
        while column <= right {
            let Some(symbol) =
                cells.get(row as usize * usize::from(frame_width) + usize::from(column))
            else {
                break;
            };
            if column >= left {
                if symbol.is_empty() {
                    text.push(' ');
                } else {
                    text.push_str(symbol);
                }
            }
            let cell_width = u16::try_from(UnicodeWidthStr::width(symbol.as_str()))
                .unwrap_or(u16::MAX)
                .max(1);
            column = column.saturating_add(cell_width);
        }
        if right == width - 1 {
            let trimmed = text[start..].trim_end_matches(' ').len();
            text.truncate(start + trimmed);
        }
    }
    text
}

impl App {
    /// The reference's `clear_popup_selection`.
    pub(super) fn clear_popup_selection(&mut self) {
        if self.popup_selection.take().is_some() {
            self.sched.mark_dirty(std::time::Instant::now());
        }
    }

    /// The reference's `handle_popup_mouse_selection`. Returns true when
    /// the event belonged to popup selection — anchors on a non-reporting
    /// left press, extends on drag, finishes (and maybe copies) on up.
    pub(super) fn handle_popup_mouse_selection(
        &mut self,
        hit: &PaneHit,
        mouse: &MouseInput,
    ) -> bool {
        if !self.shell.mouse_capture {
            self.clear_popup_selection();
            return false;
        }
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left)
                if !popup_mouse_reporting(self, hit)
                    && contains(hit.inner_rect, (mouse.col, mouse.row)) =>
            {
                self.popup_selection = Some(Selection::anchor(
                    hit.pane_id.clone(),
                    mouse.row.saturating_sub(hit.inner_rect.y),
                    mouse.col.saturating_sub(hit.inner_rect.x),
                    None,
                ));
                self.sched.mark_dirty(std::time::Instant::now());
                true
            }
            MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) => {
                let finished = {
                    let Some(selection) = self.popup_selection.as_mut().filter(|selection| {
                        selection.pane_id == hit.pane_id && selection.is_in_progress()
                    }) else {
                        return false;
                    };
                    selection.drag(mouse.col, mouse.row, hit.inner_rect, None);
                    if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
                        Some(selection.finish())
                    } else {
                        self.sched.mark_dirty(std::time::Instant::now());
                        None
                    }
                };
                if let Some(dragged) = finished {
                    if dragged && self.shell.copy_on_select {
                        self.copy_popup_selection();
                        self.clear_popup_selection();
                    } else if !dragged {
                        self.clear_popup_selection();
                    }
                    self.sched.mark_dirty(std::time::Instant::now());
                }
                true
            }
            MouseEventKind::Down(_)
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => {
                self.clear_popup_selection();
                false
            }
            _ => false,
        }
    }

    /// The reference's `copy_popup_selection`: extract the clipped text
    /// cell by cell — wide heads emit their grapheme once, blanks emit a
    /// space, and lines that reach the right edge lose trailing spaces —
    /// then write it through the OSC 52 host sink.
    pub(super) fn copy_popup_selection(&mut self) -> bool {
        let Some(selection) = self.popup_selection.clone() else {
            return false;
        };
        let terminal = selection.pane_id.clone();
        if self
            .popup
            .as_ref()
            .is_none_or(|popup| popup.terminal != terminal)
        {
            return false;
        }
        let Some(hit) = self
            .hit_map
            .popup
            .as_ref()
            .filter(|hit| hit.pane_id == terminal)
            .cloned()
        else {
            return false;
        };
        let Some((frame_width, height, cells)) = self.popup.as_mut().and_then(popup_frame_cells)
        else {
            return false;
        };
        let Some(clipped) =
            visible_selection(&selection, frame_width, height, &cells, hit.inner_rect)
        else {
            return false;
        };
        let width = frame_width.min(hit.inner_rect.width);
        let text = selection_text(&cells, frame_width, width, &clipped);
        let sequence = crate::selection::osc52_sequence(text.as_bytes());
        let _ = (self.host_control)(sequence.as_bytes());
        if self.show_copy_feedback(std::time::Instant::now()) {
            self.sched.mark_dirty(std::time::Instant::now());
        }
        true
    }

    /// The reference's `render_popup_selection`: paint the selection
    /// highlight style over the clipped cells inside the popup's inner
    /// rect.
    pub(super) fn render_popup_selection(
        &mut self,
        buf: &mut ratatui::buffer::Buffer,
        inner: Rect,
    ) {
        let Some(selection) = self
            .popup_selection
            .as_ref()
            .filter(|selection| selection.is_visible())
            .cloned()
        else {
            return;
        };
        let Some((width, height, cells)) = self
            .popup
            .as_mut()
            .filter(|popup| popup.terminal == selection.pane_id)
            .and_then(popup_frame_cells)
        else {
            return;
        };
        let Some(clipped) = visible_selection(&selection, width, height, &cells, inner) else {
            return;
        };
        let style = crate::chrome::selection_highlight::selection_style(
            &self.shell.palette,
            &self.host_facts.theme,
        );
        for y in 0..inner.height {
            for x in 0..inner.width {
                if clipped.contains(y, x, None) {
                    buf[(inner.x + x, inner.y + y)].set_style(style);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `width`-strided frame cells: each row padded to `width` cells,
    /// text at column 0 like the popup terminal lays it out.
    fn frame_cells(width: u16, rows: &[&str]) -> Vec<String> {
        let mut cells = Vec::new();
        for row in rows {
            for col in 0..width {
                cells.push(
                    row.chars()
                        .nth(usize::from(col))
                        .map(|c| c.to_string())
                        .unwrap_or_default(),
                );
            }
        }
        cells
    }

    /// A popup terminal wider than its drawn surface (the reference's
    /// `terminal_width` asymmetry): extraction must still index cells by
    /// the frame stride or it reads the wrong row. Regression for the
    /// clipped-width indexing that produced shifted copies.
    #[test]
    fn selection_text_indexes_by_frame_stride_not_clip_width() {
        let cells = frame_cells(10, &["          ", "MARK popup", "          "]);
        let clipped = Selection::absolute_range((), (1, 0), (1, 9));
        assert_eq!(selection_text(&cells, 10, 3, &clipped), "MARK popup");
    }

    /// The whole `visible_selection` + extract path under a mismatched
    /// stride: a row-1 drag over "MARK popup" yields that text, not the
    /// previous row's tail.
    #[test]
    fn visible_selection_extracts_the_dragged_row_under_wide_frame() {
        let cells = frame_cells(10, &["          ", "MARK popup", "          "]);
        let inner = Rect::new(0, 0, 3, 3);
        let mut selection = Selection::anchor("t9".to_string(), 1, 0, None);
        selection.drag(2, 1, inner, None);
        let clipped = visible_selection(&selection, 10, 3, &cells, inner).unwrap();
        let ((fr, fc), (lr, lc)) = clipped.ordered_cells();
        assert_eq!((fr, fc, lr, lc), (1, 0, 1, 2));
        let width = 10u16.min(inner.width);
        assert_eq!(selection_text(&cells, 10, width, &clipped), "MAR");
    }
}

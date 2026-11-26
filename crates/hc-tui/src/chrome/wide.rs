//! Shared wide-grapheme buffer cell helpers: tail-slot `Skip` marking
//! for terminal emission and boundary blanking before overlays draw.

use ratatui::buffer::{Buffer, Cell, CellDiffOption, CellWidth};
use ratatui::layout::Rect;
use ratatui::widgets::{Clear, Widget};

#[inline]
pub(crate) fn clear_diff_skip(cell: &mut Cell) {
    if matches!(cell.diff_option, CellDiffOption::Skip) {
        cell.set_diff_option(CellDiffOption::None);
        #[allow(deprecated)]
        {
            cell.set_skip(false);
        }
    }
}

#[inline]
fn step_wide_coverage(
    last_head: &mut Option<(u16, u16)>,
    x: u16,
    cell_width: u16,
) -> Option<(u16, u16)> {
    if let Some((hx, end)) = *last_head
        && x < end
    {
        return Some((hx, end));
    }
    if cell_width > 1 {
        let span = (x, x.saturating_add(cell_width));
        *last_head = Some(span);
        Some(span)
    } else {
        *last_head = None;
        None
    }
}

/// Walk `y` up to `target_x`, returning `Some((head_x, end_x))` if `target_x`
/// falls within `[head_x, end_x)` of a multi-column grapheme (`end_x - head_x > 1`).
fn wide_glyph_at(buf: &Buffer, target_x: u16, y: u16) -> Option<(u16, u16)> {
    if y < buf.area.top()
        || y >= buf.area.bottom()
        || target_x < buf.area.left()
        || target_x >= buf.area.right()
    {
        return None;
    }
    let mut last_head = None;
    let mut hit = None;
    for x in buf.area.left()..=target_x {
        let w = buf[(x, y)].cell_width();
        hit = step_wide_coverage(&mut last_head, x, w);
    }
    hit
}

#[inline]
fn blank_cell_keep_style(cell: &mut Cell) {
    cell.set_symbol(" ");
    clear_diff_skip(cell);
}

/// Blank any wide grapheme straddling the left or right edge of `rect`
/// before painting an overlay or shadow across `rect`:
/// - Left edge: if `rect.left()` cuts a wide head at `hx < rect.left()`,
///   blank the head (and its covered slots) keeping each cell's existing
///   (typically already-dimmed) style so the terminal never draws across
///   the border or gutter.
/// - Right edge: if `rect.right()` is a tail of a wide head inside
///   `rect`, blank the outside tail (`rect.right()..end`). When
///   `blank_inside_right` is true (e.g. a shadow pass that preserves
///   non-straddling symbols rather than overwriting with border glyphs),
///   blank the head columns inside `rect` as well so the glyph never
///   splits across two background tones.
pub(crate) fn blank_wide_edges(buf: &mut Buffer, rect: Rect, blank_inside_right: bool) {
    let clipped = rect.intersection(buf.area);
    if clipped.is_empty() {
        return;
    }
    let left = clipped.left();
    let right = clipped.right();
    for y in clipped.top()..clipped.bottom() {
        if let Some((head_x, end_x)) = wide_glyph_at(buf, left, y)
            && head_x < left
        {
            for x in head_x..end_x.min(buf.area.right()) {
                blank_cell_keep_style(&mut buf[(x, y)]);
            }
        }
        if right < buf.area.right() {
            if let Some((head_x, end_x)) = wide_glyph_at(buf, right, y)
                && head_x < right
            {
                let start = if blank_inside_right {
                    head_x.max(left)
                } else {
                    right
                };
                for x in start..end_x.min(buf.area.right()) {
                    blank_cell_keep_style(&mut buf[(x, y)]);
                }
            } else if matches!(buf[(right, y)].diff_option, CellDiffOption::Skip) {
                blank_cell_keep_style(&mut buf[(right, y)]);
            }
        }
    }
}

/// Sanitize wide-grapheme boundary cells around `area` and `Clear` it.
pub(crate) fn clear_overlay(buf: &mut Buffer, area: Rect) {
    blank_wide_edges(buf, area, false);
    Clear.render(area, buf);
}

/// Mark covered wide-grapheme tail cells `Skip` across an entire buffer
/// and clear stale `Skip` marks on uncovered cells.
pub(crate) fn mark_wide_tail_slots(buf: &mut Buffer) {
    for y in buf.area.top()..buf.area.bottom() {
        mark_row_wide_tail_slots(buf, y);
    }
}

/// Mark covered wide-grapheme tail cells `Skip` on row `y` and clear
/// stale `Skip` marks on uncovered cells.
pub(crate) fn mark_row_wide_tail_slots(buf: &mut Buffer, y: u16) {
    if y < buf.area.top() || y >= buf.area.bottom() {
        return;
    }
    let mut last_head = None;
    for x in buf.area.left()..buf.area.right() {
        let cell = &mut buf[(x, y)];
        let w = cell.cell_width();
        match step_wide_coverage(&mut last_head, x, w) {
            Some((hx, _)) if x > hx => {
                cell.set_diff_option(CellDiffOption::Skip);
            }
            _ => clear_diff_skip(cell),
        }
    }
}

//! Damage-aware terminal painting (`r2-load-lag.md` R2-T4): each pane's
//! VT feeds a `PaneRenderCache`, and only the cache's cells reach the
//! frame. A clean VT costs zero cell reads — its cached surface replays
//! through the ratatui buffer swap; a dirty one repaints just the rows
//! the render state reports. `cells_read` counts the VT cells copied
//! out so `Probe::cells_painted` can prove the damage path end to end.
//!
//! VT cells map to the cache by *physical column index* — one cell per
//! column including spacer tails, so a wide head paints once and its
//! tail cell simply isn't written; `mark_row_wide_tail_slots` marks it
//! `Skip` inside the cache, and the blit carries the mark into the
//! frame.

use super::super::pane::PaneRenderCache;
use super::*;

/// `draw_terminal` kept for overlay callers (popup/peek) that don't
/// report cell counts — the visible-pane loop calls the counted form.
pub(in crate::app) fn draw_terminal(
    pane: &mut super::super::pane::Pane,
    inner: Rect,
    buf: &mut Buffer,
    now: Instant,
    suppressed: bool,
) -> CursorWish {
    draw_terminal_counted(pane, inner, buf, now, suppressed).0
}

/// Paint `pane`'s VT through its render cache into `buf` at `inner`.
/// Returns the cursor wish, the VT cells copied out this call, and
/// whether a live VT painted (vs. the no-attach status label).
pub(super) fn draw_terminal_counted(
    pane: &mut super::super::pane::Pane,
    inner: Rect,
    buf: &mut Buffer,
    now: Instant,
    suppressed: bool,
) -> (CursorWish, u64, bool) {
    let no_cursor = CursorWish {
        pos: None,
        anchor: None,
        shape: CursorShape::Default,
        blinking: true,
    };
    if inner.is_empty() {
        return (no_cursor, 0, false);
    }
    let (paint, vt_cache) = pane.vt_and_cache();
    let Some((vt, cache)) = vt_cache else {
        // No live VT: the attach is opening/down/gone — say so instead
        // of leaving a black hole. Queued input counts ride along.
        let (pending, dropped) = pane.input_backlog();
        let mut label = pane.status_label(now);
        if pending > 0 || dropped > 0 {
            label.push_str(&format!(" · {pending} queued, {dropped} dropped"));
        }
        Paragraph::new(label)
            .style(Style::default().fg(Color::DarkGray))
            .render(Rect::new(inner.x, inner.y, inner.width, 1), buf);
        return (no_cursor, 0, false);
    };
    let geometry_ok = cache.as_ref().is_some_and(|c| {
        c.vt_id == vt.id() && c.width == inner.width && c.height == inner.height && c.paint == paint
    });
    let Ok((frame, cells_read)) =
        vt.viewport_frame(inner.width, inner.height, !geometry_ok, |colors| {
            cache.as_ref().is_some_and(|c| &c.colors == colors)
        })
    else {
        return (no_cursor, 0, false);
    };

    // A full frame (or a resized/missing cache) repaints every row; a
    // partial frame resets then repaints only its dirty rows so shrunk
    // content never leaves stale cells behind.
    let rebuild = frame.dirty == hc_vt::Dirty::Full
        || cache
            .as_ref()
            .is_none_or(|c| c.width != inner.width || c.height != inner.height);
    if rebuild {
        let surface = cache.get_or_insert_with(|| surface_for(vt.id(), &frame, &paint, inner));
        if surface.width != inner.width || surface.height != inner.height {
            surface.buf = Buffer::empty(Rect::new(0, 0, inner.width, inner.height));
            surface.width = inner.width;
            surface.height = inner.height;
        } else {
            surface.buf.reset();
        }
        for row in &frame.rows {
            if row.y < 0 || row.y >= i32::from(inner.height) {
                continue;
            }
            paint_row(&mut surface.buf, row, &frame, &paint, inner.width);
            mark_row_wide_tail_slots(&mut surface.buf, row.y as u16);
        }
        surface.vt_id = vt.id();
        surface.paint = paint;
        surface.colors = frame.colors.clone();
    } else if !frame.rows.is_empty() {
        let surface = cache.as_mut().expect("cache hit without a cache");
        for row in &frame.rows {
            if row.y < 0 || row.y >= i32::from(inner.height) {
                continue;
            }
            let ry = row.y as u16;
            // Rewritten rows drop their stale cells — a shrunk row or a
            // narrowed wide grapheme must not keep old paint or marks.
            for x in 0..inner.width {
                surface.buf[(x, ry)].reset();
            }
            paint_row(&mut surface.buf, row, &frame, &paint, inner.width);
            mark_row_wide_tail_slots(&mut surface.buf, ry);
        }
        surface.vt_id = vt.id();
        surface.paint = paint;
        surface.colors = frame.colors.clone();
    }
    let _ = vt.clean();

    // Replay the cached surface: every cell, `Skip` marks included,
    // arrives in one slice copy per row — no FFI, no width rescans.
    let cache = cache.as_ref().expect("surface built above");
    let blit_w = inner
        .width
        .min(cache.buf.area.width)
        .min(buf.area.width.saturating_sub(inner.x));
    let blit_h = inner
        .height
        .min(cache.buf.area.height)
        .min(buf.area.height.saturating_sub(inner.y));
    for ry in 0..blit_h {
        let src = usize::from(ry) * usize::from(cache.buf.area.width);
        let dst = usize::from(inner.y + ry) * usize::from(buf.area.width) + usize::from(inner.x);
        let span = usize::from(blit_w);
        buf.content[dst..dst + span].clone_from_slice(&cache.buf.content[src..src + span]);
    }

    let pos = if !suppressed && frame.cursor.visible {
        frame
            .cursor
            .position
            .filter(|(cx, cy)| *cx < inner.width && *cy < inner.height)
    } else {
        None
    };
    (
        CursorWish {
            pos,
            anchor: (!suppressed).then_some(frame.cursor.position).flatten(),
            shape: if !suppressed
                && frame.cursor.position.is_some()
                && frame.cursor.shape_overridden
            {
                frame.cursor.shape
            } else {
                CursorShape::Default
            },
            blinking: frame.cursor.blinking,
        },
        cells_read,
        true,
    )
}

fn surface_for(vt_id: u64, frame: &hc_vt::Frame, paint: &Paint, inner: Rect) -> PaneRenderCache {
    PaneRenderCache {
        vt_id,
        width: inner.width,
        height: inner.height,
        paint: *paint,
        colors: frame.colors.clone(),
        buf: Buffer::empty(Rect::new(0, 0, inner.width, inner.height)),
    }
}

/// Paint one VT row into a pane-local cache buffer: physical columns,
/// spacer tails unwritten, wide heads blanked at the clip edge.
fn paint_row(
    surface: &mut Buffer,
    row: &hc_vt::Row,
    frame: &hc_vt::Frame,
    paint: &Paint,
    max_cols: u16,
) {
    let y = row.y as u16;
    for (col, cell) in row.cells.iter().enumerate() {
        if col >= usize::from(max_cols) {
            break;
        }
        match cell.wide {
            // Covered by the wide head at col-1: paint nothing; the row
            // scan marks the slot Skip inside the cache.
            Wide::SpacerTail => continue,
            // A wide grapheme at the clip edge can't show half —
            // blank it rather than bleed over the border.
            Wide::Wide if col + 1 >= usize::from(max_cols) => {
                surface.set_string(col as u16, y, " ", cell_style(frame, cell, paint));
                continue;
            }
            _ => {}
        }
        surface.set_string(
            col as u16,
            y,
            if cell.text.is_empty() {
                " "
            } else {
                &cell.text
            },
            cell_style(frame, cell, paint),
        );
    }
}

/// Wide-grapheme tail slots: a cell covered by a multi-cell head in the
/// final composite is marked `CellDiffOption::Skip`. The option
/// participates in `Cell::eq`, so the slot stays suppressed while the head
/// covers it but becomes a real diff once the head narrows — the reference
/// emitter's invalidated-width rule (render_ansi.rs `affected_width`),
/// without which an orphaned host cell keeps stale attributes. Marking
/// after compositing — rather than while panes draw — keeps overlay writes
/// over a tail slot emit-ready; stale marks on uncovered cells are cleared
/// in the same pass.
pub(in crate::app) use crate::chrome::wide::{mark_row_wide_tail_slots, mark_wide_tail_slots};

/// `mark_wide_tail_slots` when no overlay painted: `clean_rects` are
/// cache-blit pane surfaces whose tail slots are already marked — the
/// walk jumps their interiors instead of re-measuring every cell.
/// `mode_bar`'s rows always scan fully: the bar and its saved cells
/// repaint over the surface each frame.
pub(in crate::app) fn mark_wide_tail_slots_skipping(
    buf: &mut Buffer,
    clean_rects: &[Rect],
    mode_bar: Option<Rect>,
) {
    let width = buf.area.width;
    for y in 0..buf.area.height {
        if mode_bar.is_some_and(|bar| y >= bar.y && y < bar.y.saturating_add(bar.height)) {
            mark_row_wide_tail_slots(buf, y);
            continue;
        }
        let mut covered_until = 0u16;
        let mut x = 0u16;
        while x < width {
            // A clean pane interior is already marked; jump it. Only a
            // rect entered at its own left edge skips — a wide head
            // covering in from the left still walks those cells.
            if x >= covered_until
                && let Some(rect) = clean_rects.iter().find(|r| {
                    r.width > 0 && r.x == x && y >= r.y && y < r.y.saturating_add(r.height)
                })
            {
                x = rect.x.saturating_add(rect.width);
                continue;
            }
            let cell = &mut buf[(x, y)];
            if x < covered_until {
                cell.set_diff_option(ratatui::buffer::CellDiffOption::Skip);
            } else {
                crate::chrome::wide::clear_diff_skip(cell);
                covered_until = covered_until.max(x + cell.cell_width());
            }
            x += 1;
        }
    }
}

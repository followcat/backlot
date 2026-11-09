// Derived from herdr crates/ghostty-vt/src/lib.rs
// `screen_text_rows_range`/`grid_ref_*` helpers (Apache-2.0).
//! Absolute screen-row cell reads for copy-mode motions and search.
//! Rows are addressed in screen coordinates (scrollback plus viewport,
//! the same space `read_text_screen` uses); each row reports its cells
//! plus the soft-wrap flags that join it into a logical line.
//!
//! Grid references are snapshots — valid only until the next mutating
//! terminal call — so every row is resolved and copied inside one call.

use std::mem;
use std::ptr;

use crate::data::Wide;
use crate::ffi;
use crate::vt::{Error, Vt};

/// The codepoint libghostty-vt writes into cells that hold a kitty image
/// placement placeholder — rendered text treats it as blank.
pub const KITTY_UNICODE_PLACEHOLDER: u32 = 0x10EEEE;

/// One screen row's cell text with its soft-wrap metadata.
#[derive(Debug, Clone)]
pub struct ScreenTextRow {
    pub cells: Vec<ScreenTextCell>,
    /// The row is joined to the next (this row ran to the last column).
    pub soft_wrapped: bool,
    /// The row continues the previous soft-wrapped row.
    pub wrap_continuation: bool,
}

/// One cell's grapheme cluster as codepoints, with its wide-cell tag.
#[derive(Debug, Clone)]
pub struct ScreenTextCell {
    pub wide: Wide,
    pub graphemes: Vec<u32>,
}

fn screen_point(x: u16, y: u32) -> ffi::Point {
    ffi::Point {
        tag: ffi::POINT_TAG_SCREEN,
        value: ffi::PointValue {
            coordinate: ffi::PointCoordinate { x, y },
        },
    }
}

fn ok(rc: i32) -> Result<(), Error> {
    if rc == ffi::SUCCESS {
        Ok(())
    } else {
        Err(Error::Ghostty(rc))
    }
}

impl Vt {
    /// Rows in the screen buffer: scrollback plus viewport.
    pub fn total_rows(&self) -> Result<usize, Error> {
        let mut out: usize = 0;
        unsafe {
            ok(ffi::ghostty_terminal_get(
                self.raw,
                ffi::DATA_TOTAL_ROWS,
                (&raw mut out).cast(),
            ))?;
        }
        Ok(out)
    }

    /// Copy the cells of `[start_row, end_row_exclusive)` in absolute
    /// screen rows. Rows outside the buffer clamp to the buffer edge.
    pub fn screen_text_rows_range(
        &self,
        start_row: usize,
        end_row_exclusive: usize,
    ) -> Result<Vec<ScreenTextRow>, Error> {
        let total_rows = self.total_rows()?;
        let start_row = start_row.min(total_rows);
        let end_row_exclusive = end_row_exclusive.min(total_rows).max(start_row);
        let (cols, _) = self.size()?;
        let mut rows = Vec::with_capacity(end_row_exclusive.saturating_sub(start_row));
        for y in start_row..end_row_exclusive {
            let Some(y) = u32::try_from(y).ok() else {
                break;
            };
            let mut grid_ref = self.grid_ref(screen_point(0, y))?;
            let (soft_wrapped, wrap_continuation) = grid_ref_wrap_state(&grid_ref)?;
            let mut cells = Vec::with_capacity(usize::from(cols));
            for x in 0..cols {
                grid_ref.x = x;
                cells.push(ScreenTextCell {
                    wide: grid_ref_wide(&grid_ref)?,
                    graphemes: grid_ref_graphemes(&grid_ref)?,
                });
            }
            rows.push(ScreenTextRow {
                cells,
                soft_wrapped,
                wrap_continuation,
            });
        }
        Ok(rows)
    }

    fn grid_ref(&self, point: ffi::Point) -> Result<ffi::GridRef, Error> {
        let mut grid_ref = ffi::GridRef {
            size: mem::size_of::<ffi::GridRef>(),
            ..Default::default()
        };
        unsafe {
            ok(ffi::ghostty_terminal_grid_ref(
                self.raw,
                point,
                &mut grid_ref,
            ))?;
        }
        Ok(grid_ref)
    }
}

fn grid_ref_graphemes(grid_ref: &ffi::GridRef) -> Result<Vec<u32>, Error> {
    let mut required = 0usize;
    let result =
        unsafe { ffi::ghostty_grid_ref_graphemes(grid_ref, ptr::null_mut(), 0, &mut required) };
    if result != ffi::OUT_OF_SPACE {
        ok(result)?;
    }
    let mut buffer = vec![0u32; required];
    if required == 0 {
        return Ok(buffer);
    }
    unsafe {
        ok(ffi::ghostty_grid_ref_graphemes(
            grid_ref,
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut required,
        ))?;
    }
    buffer.truncate(required);
    Ok(buffer)
}

fn grid_ref_wide(grid_ref: &ffi::GridRef) -> Result<Wide, Error> {
    let mut raw: u64 = 0;
    unsafe {
        ok(ffi::ghostty_grid_ref_cell(grid_ref, &mut raw))?;
    }
    let mut wide: i32 = ffi::WIDE_NARROW;
    unsafe {
        ok(ffi::ghostty_cell_get(
            raw,
            ffi::CELL_DATA_WIDE,
            (&raw mut wide).cast(),
        ))?;
    }
    Ok(match wide {
        ffi::WIDE_WIDE => Wide::Wide,
        ffi::WIDE_SPACER_TAIL => Wide::SpacerTail,
        ffi::WIDE_SPACER_HEAD => Wide::SpacerHead,
        _ => Wide::Narrow,
    })
}

fn grid_ref_wrap_state(grid_ref: &ffi::GridRef) -> Result<(bool, bool), Error> {
    let mut row: ffi::Row = 0;
    unsafe {
        ok(ffi::ghostty_grid_ref_row(grid_ref, &mut row))?;
    }
    let mut soft_wrapped = false;
    let mut wrap_continuation = false;
    unsafe {
        ok(ffi::ghostty_row_get(
            row,
            ffi::ROW_DATA_WRAP,
            (&raw mut soft_wrapped).cast(),
        ))?;
        ok(ffi::ghostty_row_get(
            row,
            ffi::ROW_DATA_WRAP_CONTINUATION,
            (&raw mut wrap_continuation).cast(),
        ))?;
    }
    Ok((soft_wrapped, wrap_continuation))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_text_rows_cover_scrollback_and_report_wraps() {
        let mut vt = Vt::new(5, 3, 1024);
        vt.feed(b"one\r\ntwo\r\nabcdefghij");
        assert_eq!(vt.total_rows().unwrap(), 4);
        let rows = vt.screen_text_rows_range(0, usize::MAX).unwrap();
        assert_eq!(rows.len(), 4);
        let line = |row: &ScreenTextRow| {
            row.cells
                .iter()
                .map(|cell| {
                    cell.graphemes
                        .first()
                        .and_then(|cp| char::from_u32(*cp))
                        .unwrap_or(' ')
                })
                .collect::<String>()
        };
        assert_eq!(line(&rows[0]), "one  ");
        assert_eq!(line(&rows[1]), "two  ");
        // The ten-cell word soft-wraps across the last two rows.
        assert!(rows[2].soft_wrapped);
        assert!(!rows[2].wrap_continuation);
        assert!(!rows[3].soft_wrapped);
        assert!(rows[3].wrap_continuation);
    }

    #[test]
    fn screen_text_rows_clamps_the_range_to_the_buffer() {
        let mut vt = Vt::new(8, 2, 1024);
        vt.feed(b"hi");
        let rows = vt.screen_text_rows_range(100, usize::MAX).unwrap();
        assert!(rows.is_empty());
        let rows = vt.screen_text_rows_range(0, usize::MAX).unwrap();
        assert_eq!(rows.len(), 2);
    }
}

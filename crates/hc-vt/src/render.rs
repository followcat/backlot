//! `Vt::frame`/`Vt::viewport_frame` copy terminal pictures into owned
//! `data.rs` values — nothing points into C memory once read.

use crate::data::{
    Cell, Colors, Cursor, Dirty, Frame, Row, Underline, Wide, cell_color, style_color,
};
use crate::ffi;
use crate::vt::{Error, Vt};

impl Vt {
    /// Snapshot the terminal. `history` requests that many overscan rows
    /// above the viewport (scrollback); they appear in `rows` with
    /// negative `y`. `0` captures the viewport only.
    pub fn frame(&mut self, history: u16) -> Result<Frame, Error> {
        if self.overscan != history {
            // A changed request costs a full redraw; unchanged is free.
            let request = ffi::Overscan {
                above: history,
                below: 0,
            };
            unsafe {
                ok(ffi::ghostty_render_state_set(
                    self.render,
                    ffi::RS_OPT_OVERSCAN,
                    (&raw const request).cast(),
                ))?;
            }
            self.overscan = history;
        }
        unsafe {
            ok(ffi::ghostty_render_state_update(self.render, self.raw))?;
        }

        let mut cursor = ffi::RenderStateCursor {
            size: size_of::<ffi::RenderStateCursor>(),
            ..zeroed()
        };
        let mut colors = ffi::RenderStateColors {
            size: size_of::<ffi::RenderStateColors>(),
            ..zeroed()
        };
        let mut overscan = ffi::Overscan { above: 0, below: 0 };
        let mut dirty = 0i32;
        let mut cols = 0u16;
        unsafe {
            self.rs_get(ffi::RS_CURSOR, (&raw mut cursor).cast())?;
            self.rs_get(ffi::RS_COLORS, (&raw mut colors).cast())?;
            self.rs_get(ffi::RS_OVERSCAN, (&raw mut overscan).cast())?;
            self.rs_get(ffi::RS_DIRTY, (&raw mut dirty).cast())?;
            ok(ffi::ghostty_terminal_get(
                self.raw,
                ffi::DATA_COLS,
                (&raw mut cols).cast(),
            ))?;
        }

        // The get writes the existing iterator handle in place; a local
        // sidesteps borrowing `self.rows` through `&self` at once.
        let mut rows = self.rows;
        self.rs_get(ffi::RS_ROW_ITERATOR, (&raw mut rows).cast())?;
        self.rows = rows;

        let mut out = Vec::with_capacity(usize::from(cols) + usize::from(overscan.above));
        while unsafe { ffi::ghostty_render_state_row_iterator_next(self.rows) } {
            out.push(self.read_row()?);
        }

        let mut cursor: Cursor = cursor.into();
        cursor.shape_overridden = self.cursor_tracker.cursor_shape_overridden();
        Ok(Frame {
            cols,
            rows: out,
            cursor,
            colors: colors.into(),
            dirty: match dirty {
                ffi::DIRTY_FULL => Dirty::Full,
                ffi::DIRTY_PARTIAL => Dirty::Partial,
                _ => Dirty::Clean,
            },
        })
    }

    /// Viewport snapshot for the draw path: like `frame(0)` but reads
    /// only what painting needs. `max_cols`/`max_rows` bound the cell
    /// reads to the visible inner rect; returns the frame plus the
    /// number of cells actually copied out. A clean render state with
    /// matching colors costs zero cell reads; a partial update reads
    /// only dirty rows; `force_full`, a full dirty, or `colors_match`
    /// reporting a palette change all rebuild every row. Cursor and
    /// colors are fresh in every case. Dirty state stays armed; the
    /// caller calls `clean` once its own cache is updated.
    pub fn viewport_frame(
        &mut self,
        max_cols: u16,
        max_rows: u16,
        force_full: bool,
        colors_match: impl FnOnce(&Colors) -> bool,
    ) -> Result<(Frame, u64), Error> {
        if self.overscan != 0 {
            // The viewport never shows scrollback; restoring zero
            // overscan triggers a full redraw inside the library.
            let request = ffi::Overscan { above: 0, below: 0 };
            unsafe {
                ok(ffi::ghostty_render_state_set(
                    self.render,
                    ffi::RS_OPT_OVERSCAN,
                    (&raw const request).cast(),
                ))?;
            }
            self.overscan = 0;
        }
        unsafe {
            ok(ffi::ghostty_render_state_update(self.render, self.raw))?;
        }

        let mut cursor = ffi::RenderStateCursor {
            size: size_of::<ffi::RenderStateCursor>(),
            ..zeroed()
        };
        let mut colors = ffi::RenderStateColors {
            size: size_of::<ffi::RenderStateColors>(),
            ..zeroed()
        };
        let mut dirty = 0i32;
        let mut cols = 0u16;
        unsafe {
            self.rs_get(ffi::RS_CURSOR, (&raw mut cursor).cast())?;
            self.rs_get(ffi::RS_COLORS, (&raw mut colors).cast())?;
            self.rs_get(ffi::RS_DIRTY, (&raw mut dirty).cast())?;
            ok(ffi::ghostty_terminal_get(
                self.raw,
                ffi::DATA_COLS,
                (&raw mut cols).cast(),
            ))?;
        }

        let colors: Colors = colors.into();
        let dirty = match dirty {
            ffi::DIRTY_FULL => Dirty::Full,
            ffi::DIRTY_PARTIAL => Dirty::Partial,
            _ => Dirty::Clean,
        };
        let mut cursor: Cursor = cursor.into();
        cursor.shape_overridden = self.cursor_tracker.cursor_shape_overridden();
        let full = force_full || dirty == Dirty::Full || !colors_match(&colors);
        if !full && dirty == Dirty::Clean {
            return Ok((
                Frame {
                    cols,
                    rows: Vec::new(),
                    cursor,
                    colors,
                    dirty: Dirty::Clean,
                },
                0,
            ));
        }

        let mut rows = self.rows;
        self.rs_get(ffi::RS_ROW_ITERATOR, (&raw mut rows).cast())?;
        self.rows = rows;

        let mut cells_read = 0u64;
        let mut out = Vec::new();
        while unsafe { ffi::ghostty_render_state_row_iterator_next(self.rows) } {
            let (y, row_dirty) = self.read_row_header()?;
            if y < 0 || y >= i32::from(max_rows) {
                continue;
            }
            if !full && !row_dirty {
                continue; // clean row inside a partial update: untouched
            }
            let cells = self.read_row_cells(usize::from(max_cols), &mut cells_read)?;
            out.push(Row {
                y,
                dirty: true,
                cells,
            });
        }
        Ok((
            Frame {
                cols,
                rows: out,
                cursor,
                colors,
                dirty: if full { Dirty::Full } else { dirty },
            },
            cells_read,
        ))
    }

    /// Current default colors + the 256-entry palette — no row or cell
    /// scan, and dirty state stays armed for the next `frame`.
    pub fn colors(&mut self) -> Result<Colors, Error> {
        unsafe {
            ok(ffi::ghostty_render_state_update(self.render, self.raw))?;
        }
        let mut colors = ffi::RenderStateColors {
            size: size_of::<ffi::RenderStateColors>(),
            ..zeroed()
        };
        self.rs_get(ffi::RS_COLORS, (&raw mut colors).cast())?;
        Ok(colors.into())
    }

    /// Mark all dirty state consumed; call after a frame is painted.
    pub fn clean(&mut self) -> Result<(), Error> {
        ok(unsafe { ffi::ghostty_render_state_clean(self.render) })
    }

    fn rs_get(&self, data: i32, out: *mut std::ffi::c_void) -> Result<(), Error> {
        ok(unsafe { ffi::ghostty_render_state_get(self.render, data, out) })
    }

    /// Copy one row — every field is owned before the next update.
    fn read_row(&mut self) -> Result<Row, Error> {
        let (y, dirty) = self.read_row_header()?;
        let mut scratch = 0u64;
        let cells = self.read_row_cells(usize::MAX, &mut scratch)?;
        Ok(Row { y, dirty, cells })
    }

    /// A row's viewport position and dirty flag — the two queries a
    /// partial scan pays for even on rows it skips.
    fn read_row_header(&mut self) -> Result<(i32, bool), Error> {
        let mut y: i32 = 0;
        let mut dirty = false;
        unsafe {
            ok(ffi::ghostty_render_state_row_get(
                self.rows,
                ffi::ROW_VIEWPORT_Y,
                (&raw mut y).cast(),
            ))?;
            ok(ffi::ghostty_render_state_row_get(
                self.rows,
                ffi::ROW_DIRTY,
                (&raw mut dirty).cast(),
            ))?;
        }
        Ok((y, dirty))
    }

    /// Copy the row's cells, at most `max_cols` of them; `cells_read`
    /// accumulates how many were actually read.
    fn read_row_cells(
        &mut self,
        max_cols: usize,
        cells_read: &mut u64,
    ) -> Result<Vec<Cell>, Error> {
        let mut cells = self.cells;
        unsafe {
            ok(ffi::ghostty_render_state_row_get(
                self.rows,
                ffi::ROW_CELLS,
                (&raw mut cells).cast(),
            ))?;
        }
        self.cells = cells;
        let mut out = Vec::new();
        while out.len() < max_cols
            && unsafe { ffi::ghostty_render_state_row_cells_next(self.cells) }
        {
            out.push(self.read_cell()?);
            *cells_read += 1;
        }
        Ok(out)
    }

    /// Copy one cell: grapheme bytes, packed-cell wide tag, style, and
    /// the resolved fg/bg (which also see packed erase-fill colors).
    fn read_cell(&mut self) -> Result<Cell, Error> {
        let mut raw: u64 = 0;
        let mut wide: i32 = ffi::WIDE_NARROW;
        let mut style = ffi::Style {
            size: size_of::<ffi::Style>(),
            ..zeroed()
        };
        let (mut fg, mut bg) = (ffi::Rgb::default(), ffi::Rgb::default());
        let (fg_rc, bg_rc) = unsafe {
            ok(ffi::ghostty_render_state_row_cells_get(
                self.cells,
                ffi::CELL_RAW,
                (&raw mut raw).cast(),
            ))?;
            ok(ffi::ghostty_cell_get(
                raw,
                ffi::CELL_DATA_WIDE,
                (&raw mut wide).cast(),
            ))?;
            ok(ffi::ghostty_render_state_row_cells_get(
                self.cells,
                ffi::CELL_STYLE,
                (&raw mut style).cast(),
            ))?;
            (
                ffi::ghostty_render_state_row_cells_get(
                    self.cells,
                    ffi::CELL_FG_COLOR,
                    (&raw mut fg).cast(),
                ),
                ffi::ghostty_render_state_row_cells_get(
                    self.cells,
                    ffi::CELL_BG_COLOR,
                    (&raw mut bg).cast(),
                ),
            )
        };
        Ok(Cell {
            text: self.grapheme()?,
            wide: match wide {
                ffi::WIDE_WIDE => Wide::Wide,
                ffi::WIDE_SPACER_TAIL => Wide::SpacerTail,
                ffi::WIDE_SPACER_HEAD => Wide::SpacerHead,
                _ => Wide::Narrow,
            },
            fg: cell_color(style.fg_color, fg, fg_rc)?,
            bg: cell_color(style.bg_color, bg, bg_rc)?,
            underline_color: style_color(style.underline_color),
            underline: match style.underline {
                1 => Underline::Single,
                2 => Underline::Double,
                3 => Underline::Curly,
                4 => Underline::Dotted,
                5 => Underline::Dashed,
                _ => Underline::None,
            },
            bold: style.bold,
            faint: style.faint,
            italic: style.italic,
            inverse: style.inverse,
            invisible: style.invisible,
            strikethrough: style.strikethrough,
            overline: style.overline,
            blink: style.blink,
        })
    }

    /// The cell's grapheme cluster. The library's `hasText`/ASCII fast
    /// path answers in one call, so the first read targets a stack
    /// buffer that covers every common cluster; only an oversized
    /// answer pays for a heap buffer and a second call.
    fn grapheme(&mut self) -> Result<String, Error> {
        // The LEN query is skipped on purpose: `graphemes_utf8` already
        // reports `hasText == false` and the required size in one call.
        const _: () = assert!(ffi::CELL_GRAPHEMES_LEN == 3);
        let mut stack = [0u8; 32];
        let mut buf = ffi::Buffer {
            ptr: stack.as_mut_ptr(),
            cap: stack.len(),
            len: 0,
        };
        let rc = unsafe {
            ffi::ghostty_render_state_row_cells_get(
                self.cells,
                ffi::CELL_GRAPHEMES_UTF8,
                (&raw mut buf).cast(),
            )
        };
        if rc == ffi::SUCCESS {
            return Ok(match std::str::from_utf8(&stack[..buf.len]) {
                Ok(text) => text.to_owned(),
                Err(_) => String::from_utf8_lossy(&stack[..buf.len]).into_owned(),
            });
        }
        if rc != ffi::OUT_OF_SPACE || buf.len == 0 {
            return Err(Error::Ghostty(rc));
        }
        // buf.len holds the required size — only multi-kilobyte
        // combining chains ever reach this branch.
        let mut out = vec![0u8; buf.len];
        buf = ffi::Buffer {
            ptr: out.as_mut_ptr(),
            cap: out.len(),
            len: 0,
        };
        let rc = unsafe {
            ffi::ghostty_render_state_row_cells_get(
                self.cells,
                ffi::CELL_GRAPHEMES_UTF8,
                (&raw mut buf).cast(),
            )
        };
        ok(rc)?;
        out.truncate(buf.len);
        Ok(String::from_utf8(out)
            .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()))
    }
}

fn zeroed<T>() -> T {
    // SAFETY: only ever used for the C sized structs in this file, whose
    // fields are all plain data — zero is a valid bit pattern, and `size`
    // is overwritten before the value is handed to C.
    unsafe { std::mem::zeroed() }
}

fn ok(rc: i32) -> Result<(), Error> {
    (rc == ffi::SUCCESS).then_some(()).ok_or(Error::Ghostty(rc))
}

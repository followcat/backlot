//! `Vt::frame` copies one full picture of the terminal into the owned
//! Rust values in `data.rs`. Reads outlive any later mutation of the
//! `Vt`; nothing here points into C memory.

use std::ptr;

use crate::data::{
    Cell, Colors, Cursor, CursorShape, Dirty, Frame, Rgb, Row, Underline, Wide, cell_color,
    style_color,
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
            colors: Colors {
                background: colors.background.into(),
                foreground: colors.foreground.into(),
                cursor: colors.cursor_has_value.then(|| colors.cursor.into()),
                palette: colors.palette.map(Rgb::from),
            },
            dirty: match dirty {
                ffi::DIRTY_FULL => Dirty::Full,
                ffi::DIRTY_PARTIAL => Dirty::Partial,
                _ => Dirty::Clean,
            },
        })
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
        let mut y: i32 = 0;
        let mut dirty = false;
        let mut cells = self.cells;
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
            ok(ffi::ghostty_render_state_row_get(
                self.rows,
                ffi::ROW_CELLS,
                (&raw mut cells).cast(),
            ))?;
        }
        self.cells = cells;
        let mut out = Vec::new();
        while unsafe { ffi::ghostty_render_state_row_cells_next(self.cells) } {
            out.push(self.read_cell()?);
        }
        Ok(Row {
            y,
            dirty,
            cells: out,
        })
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

    /// The cell's grapheme cluster, sized by the library — never a
    /// fixed buffer (long combining sequences are common in emoji).
    fn grapheme(&mut self) -> Result<String, Error> {
        let mut len: u32 = 0;
        unsafe {
            ok(ffi::ghostty_render_state_row_cells_get(
                self.cells,
                ffi::CELL_GRAPHEMES_LEN,
                (&raw mut len).cast(),
            ))?;
        }
        if len == 0 {
            return Ok(String::new());
        }
        let mut buf = ffi::Buffer {
            ptr: ptr::null_mut(),
            cap: 0,
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
            return Ok(String::new()); // len==0: no text, per the ABI contract
        }
        if rc != ffi::OUT_OF_SPACE || buf.len == 0 {
            return Err(Error::Ghostty(rc));
        }
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
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
}

impl From<ffi::RenderStateCursor> for Cursor {
    fn from(c: ffi::RenderStateCursor) -> Cursor {
        Cursor {
            position: c.viewport_has_value.then_some((c.viewport_x, c.viewport_y)),
            visible: c.visible,
            blinking: c.blinking,
            password_input: c.password_input,
            wide_tail: c.wide_tail,
            shape_overridden: false,
            shape: match c.visual_style {
                ffi::CURSOR_BAR => CursorShape::Bar,
                ffi::CURSOR_UNDERLINE => CursorShape::Underline,
                ffi::CURSOR_BLOCK_HOLLOW => CursorShape::BlockHollow,
                _ => CursorShape::Block,
            },
        }
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

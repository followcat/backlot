// Derived from herdr src/protocol/render_ansi.rs (Apache-2.0).
//! Ratatui diff adapter preserving Source's full modifier bits and SGR
//! styles. Frame bytes accumulate in `frame` and are emitted by
//! `Backend::flush` as one write: synchronized output, hide cursor,
//! changed cells, final cursor position/shape/visibility inside the
//! block, sync end, then the reference's IME anchor repeat. A flush
//! whose diff is empty and whose cursor state is unchanged emits
//! nothing — identical frames never reach the terminal.
use std::io::{self, Write};

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use unicode_width::UnicodeWidthStr;

mod style;

/// The resolved outer-cursor state for one frame — the reference's
/// `HostCursorState` (position, visibility, DECSCUSR shape).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostCursor {
    /// 0-based, already clamped to the screen.
    pub x: u16,
    /// 0-based, already clamped to the screen.
    pub y: u16,
    /// Whether the host cursor is visible at `x`,`y`.
    pub visible: bool,
    /// DECSCUSR parameter (0-6); 0 keeps the terminal's own shape.
    pub shape: u8,
}

/// How a backend receives the frame's host cursor and reports what its
/// last `Backend::flush` emitted. `AnsiBackend` emits the cursor inside
/// the synchronized block; other backends ignore it and always report
/// an emission so scheduling is unchanged.
pub trait FrameCursor {
    /// The cursor the upcoming buffer apply should end at. The app sets
    /// it between composing the frame and applying the buffer.
    fn set_frame_cursor(&mut self, _cursor: HostCursor) {}
    /// Whether the most recent `Backend::flush` wrote frame bytes.
    /// Defaults to `true` so non-ANSI backends keep counting draws.
    fn emitted(&self) -> bool {
        true
    }
}

impl FrameCursor for ratatui::backend::TestBackend {}

/// The DECSCUSR parameter for an inner-program cursor shape — the same
/// numbering the reference's `normalize_cursor_shape` passes through.
pub(crate) fn cursor_shape_param(shape: hc_vt::CursorShape, blinking: bool) -> u8 {
    use hc_vt::CursorShape;
    match (shape, blinking) {
        (CursorShape::Default, _) => 0,
        (CursorShape::Block | CursorShape::BlockHollow, true) => 1,
        (CursorShape::Block | CursorShape::BlockHollow, false) => 2,
        (CursorShape::Underline, true) => 3,
        (CursorShape::Underline, false) => 4,
        (CursorShape::Bar, true) => 5,
        (CursorShape::Bar, false) => 6,
    }
}

/// Cell output uses the reference SGR path, buffered into `frame` and
/// emitted atomically by `flush`; terminal queries go to crossterm.
pub struct AnsiBackend<W: Write> {
    /// The frame's byte sink (stdout in production).
    out: W,
    /// Bytes of the in-flight frame.
    frame: Vec<u8>,
    /// Cells written into `frame` by the current `draw`.
    cells: usize,
    /// Whether the current frame emitted any SGR — gates the trailing
    /// `0m`, matching the reference's `sgr` flag.
    styled: bool,
    /// Whether the next frame owes a `2J` — armed by `clear` and
    /// `clear_region(All)` (ratatui calls them on init and resize) so
    /// the clear lands inside the sync block instead of flashing a
    /// blank frame between frames. Starts armed: the first emitted
    /// frame clears, matching `clear_before_full_redraw`.
    pending_clear: bool,
    /// Whether the current frame carries a `2J` — a clear requested
    /// between frames must still emit even when the diff is empty.
    frame_clear: bool,
    /// The cursor state `flush` should end the current frame at.
    cursor: Option<HostCursor>,
    /// The last emitted cursor state — dedups shape writes and skips
    /// frames whose only change is nothing at all.
    emitted_cursor: Option<HostCursor>,
    /// Whether the last `flush` wrote bytes.
    emitted: bool,
}

impl<W: Write> AnsiBackend<W> {
    pub fn new(writer: W) -> Self {
        Self {
            out: writer,
            frame: Vec::new(),
            cells: 0,
            styled: false,
            pending_clear: true,
            frame_clear: false,
            cursor: None,
            emitted_cursor: None,
            emitted: false,
        }
    }
}

impl<W: Write> FrameCursor for AnsiBackend<W> {
    fn set_frame_cursor(&mut self, cursor: HostCursor) {
        self.cursor = Some(cursor);
    }
    fn emitted(&self) -> bool {
        self.emitted
    }
}

impl<W: Write> Backend for AnsiBackend<W> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.frame.clear();
        // Synchronized output + hide + OSC 8 reset — the reference's
        // `blit_frame_to` header; `2J` when a clear is owed, matching
        // `clear_before_full_redraw`.
        self.frame
            .extend_from_slice(b"\x1b[?2026h\x1b[?25l\x1b]8;;\x1b\\");
        self.frame_clear = std::mem::take(&mut self.pending_clear);
        if self.frame_clear {
            self.frame.extend_from_slice(b"\x1b[2J");
        }
        self.cells = 0;
        self.styled = false;
        let mut next = None;
        let mut last_style = None;
        for (x, y, cell) in content {
            if cell.diff_option == ratatui::buffer::CellDiffOption::Skip {
                continue;
            }
            if next != Some((x, y)) {
                let _ = write!(&mut self.frame, "\x1b[{};{}H", y + 1, x + 1);
            }
            let style = (
                style::color_to_u32(cell.fg),
                style::color_to_u32(cell.bg),
                cell.modifier.bits(),
            );
            if last_style != Some(style) {
                let _ = self
                    .frame
                    .write_all(style::build_sgr(style.0, style.1, style.2).as_bytes());
                self.styled = true;
                last_style = Some(style);
            }
            let _ = self.frame.write_all(cell.symbol().as_bytes());
            self.cells += 1;
            next = Some((x.saturating_add(cell.symbol().width() as u16), y));
        }
        if self.styled {
            self.frame.extend_from_slice(b"\x1b[0m");
        }
        Ok(())
    }

    fn append_lines(&mut self, n: u16) -> io::Result<()> {
        // Inline-viewport only; crossterm's reference impl does the same.
        for _ in 0..n {
            self.out.write_all(b"\n")?;
        }
        self.out.flush()
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        // The frame's own hide/show runs inside the sync block — a
        // stray hide here would flash the cursor between frames.
        Ok(())
    }
    fn show_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn get_cursor_position(&mut self) -> io::Result<Position> {
        crossterm::cursor::position()
            .map(|(x, y)| Position { x, y })
            .map_err(io::Error::other)
    }
    fn set_cursor_position<P: Into<Position>>(&mut self, _position: P) -> io::Result<()> {
        // Mid-frame CUP writes are owned by `draw`; the final position
        // is emitted by `flush` from `set_frame_cursor`.
        Ok(())
    }
    fn clear(&mut self) -> io::Result<()> {
        // A clear outside a sync block presents a blank frame — the
        // reference clears inside the next one instead.
        self.pending_clear = true;
        Ok(())
    }
    fn clear_region(&mut self, kind: ClearType) -> io::Result<()> {
        // Resize lands here as `All`: the repaint covers the new grid,
        // but cells past a shrunken edge linger without a real clear.
        if matches!(kind, ClearType::All) {
            self.pending_clear = true;
        }
        Ok(())
    }
    fn size(&self) -> io::Result<Size> {
        let (width, height) = crossterm::terminal::size()?;
        Ok(Size { width, height })
    }
    fn window_size(&mut self) -> io::Result<WindowSize> {
        let crossterm::terminal::WindowSize {
            columns,
            rows,
            width,
            height,
        } = crossterm::terminal::window_size()?;
        Ok(WindowSize {
            columns_rows: Size {
                width: columns,
                height: rows,
            },
            pixels: Size { width, height },
        })
    }
    fn flush(&mut self) -> io::Result<()> {
        let Some(cursor) = self.cursor.take() else {
            // No frame in flight (e.g. Terminal drop): nothing buffered.
            self.emitted = false;
            return Ok(());
        };
        if self.cells == 0 && !self.frame_clear && self.emitted_cursor == Some(cursor) {
            self.frame.clear();
            self.emitted = false;
            return Ok(());
        }
        // Final cursor state inside the sync block — the reference's
        // `write_host_cursor_state` (shape only on change), then sync
        // end and the IME anchor repeat.
        let _ = write!(self.frame, "\x1b[{};{}H", cursor.y + 1, cursor.x + 1);
        if self.emitted_cursor.map(|c| c.shape).unwrap_or(0) != cursor.shape {
            let _ = write!(self.frame, "\x1b[{} q", cursor.shape);
        }
        let vis: &[u8] = if cursor.visible {
            b"\x1b[?25h"
        } else {
            b"\x1b[?25l"
        };
        self.frame.extend_from_slice(vis);
        self.frame.extend_from_slice(b"\x1b[?2026l");
        let _ = write!(self.frame, "\x1b[{};{}H", cursor.y + 1, cursor.x + 1);
        self.frame.extend_from_slice(vis);
        self.out.write_all(&self.frame)?;
        self.out.flush()?;
        self.emitted_cursor = Some(cursor);
        self.emitted = true;
        Ok(())
    }
}

#[cfg(test)]
#[path = "ansi/tests.rs"]
mod tests;

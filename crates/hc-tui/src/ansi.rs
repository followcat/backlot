// Derived from herdr src/protocol/render_ansi.rs (Apache-2.0).
//! Ratatui diff adapter preserving Source's full modifier bits and SGR styles.
use std::io::{self, Write};

use ratatui::backend::{Backend, ClearType, CrosstermBackend, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use unicode_width::UnicodeWidthStr;

mod style;

/// Crossterm handles terminal control; cell output uses the reference SGR path.
pub struct AnsiBackend<W: Write>(CrosstermBackend<W>);

impl<W: Write> AnsiBackend<W> {
    pub fn new(writer: W) -> Self {
        Self(CrosstermBackend::new(writer))
    }
}

impl<W: Write> Backend for AnsiBackend<W> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.0.write_all(b"\x1b[?2026h\x1b[?25l")?;
        let mut next = None;
        let mut last_style = None;
        for (x, y, cell) in content {
            if cell.diff_option == ratatui::buffer::CellDiffOption::Skip {
                continue;
            }
            if next != Some((x, y)) {
                write!(self.0, "\x1b[{};{}H", y + 1, x + 1)?;
            }
            let style = (
                style::color_to_u32(cell.fg),
                style::color_to_u32(cell.bg),
                cell.modifier.bits(),
            );
            if last_style != Some(style) {
                self.0
                    .write_all(style::build_sgr(style.0, style.1, style.2).as_bytes())?;
                last_style = Some(style);
            }
            self.0.write_all(cell.symbol().as_bytes())?;
            next = Some((x.saturating_add(cell.symbol().width() as u16), y));
        }
        self.0.write_all(b"\x1b[0m\x1b[?2026l")
    }

    fn append_lines(&mut self, n: u16) -> io::Result<()> {
        self.0.append_lines(n)
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        self.0.hide_cursor()
    }
    fn show_cursor(&mut self) -> io::Result<()> {
        self.0.show_cursor()
    }
    fn get_cursor_position(&mut self) -> io::Result<Position> {
        self.0.get_cursor_position()
    }
    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.0.set_cursor_position(position)
    }
    fn clear(&mut self) -> io::Result<()> {
        self.0.clear()
    }
    fn clear_region(&mut self, kind: ClearType) -> io::Result<()> {
        self.0.clear_region(kind)
    }
    fn size(&self) -> io::Result<Size> {
        self.0.size()
    }
    fn window_size(&mut self) -> io::Result<WindowSize> {
        self.0.window_size()
    }
    fn flush(&mut self) -> io::Result<()> {
        Backend::flush(&mut self.0)
    }
}

#[cfg(test)]
#[path = "ansi/tests.rs"]
mod tests;

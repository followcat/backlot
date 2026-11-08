// Derived from herdr-core crates/ghostty-vt/src/lib.rs (Apache-2.0).
//! Absolute screen-row text reads for local selection/copy. Screen
//! coordinates cover scrollback plus the viewport, so a selection anchored
//! before new output keeps naming the same rows. Reads go through the
//! library's formatter: `unwrap` keeps soft-wrapped rows joined and `trim`
//! drops trailing blanks, matching the reference's selection extraction.
//!
//! Grid references are snapshots — valid only until the next mutating
//! terminal call — so every read resolves and formats inside one call.

use std::mem;
use std::ptr;
use std::slice;

use crate::ffi;
use crate::vt::{Error, Vt};

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
    /// Read the inclusive `(start, end)` range in absolute screen rows.
    /// `rectangle` keeps column bounds per row instead of a linear span.
    /// Returns an error when either endpoint is out of bounds.
    pub fn read_text_screen(
        &self,
        start: (u16, u32),
        end: (u16, u32),
        rectangle: bool,
    ) -> Result<String, Error> {
        self.read_formatted_selection(
            screen_point(start.0, start.1),
            screen_point(end.0, end.1),
            rectangle,
        )
    }

    /// One absolute screen row's text, trailing blanks trimmed — the row
    /// read double-click word selection expands from.
    pub fn screen_row_text(&self, row: u32, cols: u16) -> Result<String, Error> {
        self.read_text_screen((0, row), (cols.saturating_sub(1), row), false)
    }

    fn read_formatted_selection(
        &self,
        start: ffi::Point,
        end: ffi::Point,
        rectangle: bool,
    ) -> Result<String, Error> {
        let mut start_ref = ffi::GridRef {
            size: mem::size_of::<ffi::GridRef>(),
            ..Default::default()
        };
        let mut end_ref = ffi::GridRef {
            size: mem::size_of::<ffi::GridRef>(),
            ..Default::default()
        };
        unsafe {
            // SAFETY: self.raw is a live terminal; out refs are fully written
            // on success and untouched otherwise.
            ok(ffi::ghostty_terminal_grid_ref(
                self.raw,
                start,
                &mut start_ref,
            ))?;
            ok(ffi::ghostty_terminal_grid_ref(self.raw, end, &mut end_ref))?;
        }

        let selection = ffi::Selection {
            size: mem::size_of::<ffi::Selection>(),
            start: start_ref,
            end: end_ref,
            rectangle,
        };
        self.format_selection(&selection)
    }

    fn format_selection(&self, selection: &ffi::Selection) -> Result<String, Error> {
        let mut formatter: ffi::Formatter = ptr::null_mut();
        let options = ffi::FormatterTerminalOptions {
            size: mem::size_of::<ffi::FormatterTerminalOptions>(),
            emit: ffi::FORMATTER_FORMAT_PLAIN,
            unwrap: true,
            trim: true,
            extra: ffi::FormatterTerminalExtra {
                size: mem::size_of::<ffi::FormatterTerminalExtra>(),
                screen: ffi::FormatterScreenExtra {
                    size: mem::size_of::<ffi::FormatterScreenExtra>(),
                    ..Default::default()
                },
                ..Default::default()
            },
            selection,
        };
        unsafe {
            // SAFETY: the terminal outlives the formatter per the API; the
            // formatter is freed before returning on every path.
            ok(ffi::ghostty_formatter_terminal_new(
                ptr::null(),
                &mut formatter,
                self.raw,
                options,
            ))?;
        }

        let mut out_ptr = ptr::null_mut();
        let mut out_len = 0usize;
        let result = unsafe {
            ffi::ghostty_formatter_format_alloc(formatter, ptr::null(), &mut out_ptr, &mut out_len)
        };
        unsafe {
            ffi::ghostty_formatter_free(formatter);
        }
        ok(result)?;

        let text = if out_len == 0 {
            String::new()
        } else {
            let bytes = unsafe { slice::from_raw_parts(out_ptr.cast_const(), out_len) };
            String::from_utf8_lossy(bytes).into_owned()
        };
        if !out_ptr.is_null() {
            unsafe {
                ffi::ghostty_free(ptr::null(), out_ptr, out_len);
            }
        }
        Ok(text)
    }
}

/// The library's per-codepoint display width (0, 1 or 2 cells) — the same
/// measure word-boundary mapping uses to translate text to columns.
pub fn codepoint_width(cp: u32) -> u8 {
    unsafe { ffi::ghostty_unicode_codepoint_width(cp) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_text_screen_spans_scrollback_and_viewport() {
        let mut vt = Vt::new(10, 3, 1024);
        // Five lines: "one"/"two" scroll into history above the viewport.
        vt.feed(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");
        assert_eq!(vt.read_text_screen((0, 0), (9, 0), false).unwrap(), "one");
        assert_eq!(
            vt.read_text_screen((0, 0), (9, 4), false).unwrap(),
            "one\ntwo\nthree\nfour\nfive"
        );
        assert_eq!(vt.read_text_screen((0, 3), (4, 3), false).unwrap(), "four");
    }

    #[test]
    fn read_text_screen_unwraps_soft_wrapped_rows() {
        let mut vt = Vt::new(5, 3, 0);
        vt.feed(b"abcdefghij"); // wraps: "abcde" + "fghij"
        assert_eq!(
            vt.read_text_screen((0, 0), (4, 1), false).unwrap(),
            "abcdefghij"
        );
    }

    #[test]
    fn screen_row_text_trims_trailing_blanks() {
        let mut vt = Vt::new(10, 3, 0);
        vt.feed(b"hi  ");
        assert_eq!(vt.screen_row_text(0, 10).unwrap(), "hi");
    }

    #[test]
    fn codepoint_width_reports_wide_and_zero_width() {
        assert_eq!(codepoint_width('a' as u32), 1);
        assert_eq!(codepoint_width('中' as u32), 2);
        assert_eq!(codepoint_width('\u{0301}' as u32), 0);
    }
}

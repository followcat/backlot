// Derived from herdr src/copy_mode.rs and src/api/schema/panes.rs
// (Apache-2.0).
//! Copy-mode shared helpers and the point/range/motion types the reference
//! sends over `pane.copy_motion`/`pane.copy_search`. hc-tui resolves the
//! same motions and searches locally against the pane's retained VT — the
//! types stay identical so behavior is indistinguishable.

use crossterm::event::{KeyCode, KeyModifiers};

use crate::config::TerminalKey;

/// A cell position in absolute screen-buffer coordinates (scrollback plus
/// viewport) — the reference's `PaneTextPoint`/`TerminalTextPoint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PaneTextPoint {
    pub row: u32,
    pub col: u16,
}

/// An inclusive cell range — the reference's `PaneTextRange`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PaneTextRange {
    pub start: PaneTextPoint,
    pub end: PaneTextPoint,
}

/// The reference's `PaneCopyMotion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopyMotion {
    LineEnd,
    FirstNonBlank,
    NextWordStart,
    PreviousWordStart,
    NextWordEnd,
    NextBigWordStart,
    PreviousBigWordStart,
    NextBigWordEnd,
    PreviousParagraph,
    NextParagraph,
}

/// The reference's `PaneCopySearchDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopySearchDirection {
    Forward,
    Backward,
}

pub(crate) fn first_non_blank_col(text: &str) -> Option<u16> {
    let mut col = 0u16;
    for ch in text.chars() {
        if !ch.is_whitespace() {
            return Some(col);
        }
        col = col.saturating_add(char_cell_width(ch));
    }
    None
}

pub(crate) fn last_character_col(text: &str) -> Option<u16> {
    let mut col = 0u16;
    let mut last_col = None;
    for ch in text.chars() {
        let width = u16::from(hc_vt::codepoint_width(ch as u32));
        if width > 0 {
            last_col = Some(col);
            col = col.saturating_add(width);
        }
    }
    last_col
}

fn char_cell_width(ch: char) -> u16 {
    u16::from(hc_vt::codepoint_width(ch as u32)).max(1)
}

pub(crate) fn copy_mode_page_lines(height: u16, half_page: bool) -> usize {
    if height <= 2 {
        1
    } else if half_page {
        usize::from(height / 2)
    } else {
        usize::from(height - 2)
    }
}

pub(crate) fn copy_mode_command_char(key: &TerminalKey) -> Option<char> {
    if !key.modifiers.difference(KeyModifiers::SHIFT).is_empty() {
        return None;
    }
    if let Some(ch) = key.shifted_codepoint.and_then(char::from_u32) {
        return Some(ch);
    }
    let KeyCode::Char(ch) = key.code else {
        return None;
    };
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        Some(shifted_ascii_char(ch).unwrap_or(ch))
    } else {
        Some(ch)
    }
}

fn shifted_ascii_char(ch: char) -> Option<char> {
    match ch {
        'a'..='z' => Some(ch.to_ascii_uppercase()),
        '1' => Some('!'),
        '2' => Some('@'),
        '3' => Some('#'),
        '4' => Some('$'),
        '5' => Some('%'),
        '6' => Some('^'),
        '7' => Some('&'),
        '8' => Some('*'),
        '9' => Some('('),
        '0' => Some(')'),
        '-' => Some('_'),
        '=' => Some('+'),
        '[' => Some('{'),
        ']' => Some('}'),
        '\\' => Some('|'),
        ';' => Some(':'),
        '\'' => Some('"'),
        ',' => Some('<'),
        '.' => Some('>'),
        '/' => Some('?'),
        '`' => Some('~'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_non_blank_col_counts_cell_widths() {
        assert_eq!(first_non_blank_col("  hi"), Some(2));
        assert_eq!(first_non_blank_col("中 x"), Some(0));
        assert_eq!(first_non_blank_col("中"), Some(0));
        assert_eq!(first_non_blank_col(" 中中 x"), Some(1));
        assert_eq!(first_non_blank_col("   "), None);
        assert_eq!(first_non_blank_col(""), None);
    }

    #[test]
    fn last_character_col_tracks_the_last_visible_cell() {
        // Trailing spaces count — the reference returns the last cell
        // holding any glyph-width text, not the last non-blank.
        assert_eq!(last_character_col("hi  "), Some(3));
        assert_eq!(last_character_col("中"), Some(0));
        assert_eq!(last_character_col("a中"), Some(1));
        assert_eq!(last_character_col("a中 "), Some(3));
        assert_eq!(last_character_col("e\u{0301}"), Some(0));
        assert_eq!(last_character_col(""), None);
    }

    #[test]
    fn copy_mode_page_lines_matches_the_reference() {
        assert_eq!(copy_mode_page_lines(24, false), 22);
        assert_eq!(copy_mode_page_lines(24, true), 12);
        assert_eq!(copy_mode_page_lines(2, false), 1);
        assert_eq!(copy_mode_page_lines(2, true), 1);
        assert_eq!(copy_mode_page_lines(1, false), 1);
        assert_eq!(copy_mode_page_lines(3, false), 1);
        assert_eq!(copy_mode_page_lines(3, true), 1);
        assert_eq!(copy_mode_page_lines(5, true), 2);
    }
}

//! Every string the API hands us is untrusted terminal output: a name,
//! title, cwd, status word or error can carry C0/C1 controls and ESC, and
//! ratatui would print them straight to the host terminal. [`clean`]
//! removes them; [`fit`] truncates to display cells on grapheme
//! boundaries, so neither escapes nor half a wide char ever render.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Strips control characters: `\t`, `\n`, `\r` flatten to spaces (they
/// separate words, not escape anything); every other control — C0, C1,
/// DEL, ESC — is dropped so it can never start a host sequence.
pub fn clean(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            '\t' | '\n' | '\r' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

/// The string, capped at `width` display cells. Cuts only between
/// grapheme clusters and ends a shortened string with `…` (dropped when
/// it would itself overflow). Width 0 yields an empty string.
pub fn fit(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    let mut cut = false;
    for g in text.graphemes(true) {
        let w = UnicodeWidthStr::width(g);
        if used + w > width {
            cut = true;
            break;
        }
        out.push_str(g);
        used += w;
    }
    if cut {
        // Make room for `…` by dropping whole graphemes — never part of
        // a cluster (a regional flag or emoji+VS16 stays whole or goes).
        while used + 1 > width {
            let Some((start, _)) = out.grapheme_indices(true).next_back() else {
                break;
            };
            used -= UnicodeWidthStr::width(&out[start..]);
            out.truncate(start);
        }
        if used < width {
            out.push('…');
        }
    }
    out
}

/// The string's END, capped at `width` display cells: `…tail`. Same
/// grapheme rules as [`fit`]; for strings whose significant part is the
/// suffix (a terminal's `…/t7` id under a long machine alias).
pub fn fit_tail(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= width {
        return text.into();
    }
    if width == 1 {
        return "…".into();
    }
    let mut tail = String::new();
    let mut used = 0;
    for g in text.graphemes(true).rev() {
        let w = UnicodeWidthStr::width(g);
        if used + w > width - 1 {
            break;
        }
        tail.insert_str(0, g);
        used += w;
    }
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_strips_esc_and_keeps_words() {
        assert_eq!(clean("a\x1b[31mb"), "a[31mb");
        assert_eq!(clean("\x07bell\0"), "bell");
        assert_eq!(clean("one\ttwo\nthree"), "one two three");
        assert_eq!(clean("\u{7f}\u{9b}x"), "x");
        assert_eq!(clean("plain"), "plain");
    }

    #[test]
    fn fit_respects_width_and_graphemes() {
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("abc", 4), "abc");
        assert_eq!(fit("", 4), "");
        assert_eq!(fit("abc", 0), "");
        assert_eq!(fit("abc", 1), "…");
        // Wide chars count double and are never halved.
        assert_eq!(fit("あいうえ", 5), "あい…");
        assert_eq!(fit("あい", 4), "あい");
        assert_eq!(fit("あ", 1), "…");
        // Combining marks stay with their base.
        assert_eq!(fit("e\u{301}fgh", 2), "e\u{301}…");
        assert_eq!(fit("a🇫🇷bc", 3), "a…");
        // Emoji+VS16 is one grapheme: it survives whole or not at all.
        assert_eq!(fit("❤\u{fe0f}ab", 3), "❤\u{fe0f}…");
        assert_eq!(fit("❤\u{fe0f}ab", 2), "…");
        // Zero-width input at width 0 still yields empty.
        assert_eq!(fit("\u{200b}", 0), "");
    }

    #[test]
    fn fit_tail_keeps_the_end_on_grapheme_bounds() {
        assert_eq!(fit_tail("abcdef", 4), "…def");
        assert_eq!(fit_tail("abc", 4), "abc");
        assert_eq!(fit_tail("abc", 0), "");
        assert_eq!(fit_tail("abc", 1), "…");
        assert_eq!(fit_tail("host-b/t7", 4), "…/t7");
        // Wide and combined graphemes are never halved.
        assert_eq!(fit_tail("abあいc", 4), "…いc");
        assert_eq!(fit_tail("abxe\u{301}t7", 4), "…e\u{301}t7");
    }
}

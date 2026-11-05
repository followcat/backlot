//! Every string the API hands us is untrusted terminal output: a name,
//! title, cwd, status word or error can carry C0/C1 controls and ESC, and
//! ratatui would print them straight to the host terminal. [`clean`]
//! removes them; the ported `chrome::text::truncate_end` owns display
//! truncation on the render path.

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
}

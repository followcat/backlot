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

/// Like [`clean`], but complete CSI (`ESC [ …` to a `0x40–0x7E` final
/// byte) and OSC (`ESC ] …` to BEL or `ESC \\`) sequences are dropped
/// whole first, so `\x1b[1;32mok\x1b[0m` shows `ok`, not `[1;32mok[0m`.
/// A truncated or unterminated sequence is not complete: its ESC drops
/// and the rest stays as plain text, exactly what `clean` alone does.
pub fn clean_ansi(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(esc) = rest.find('\x1b') {
        kept.push_str(&rest[..esc]);
        let after = &rest[esc + 1..];
        rest = after;
        if let Some(csi) = after.strip_prefix('[') {
            match csi_tail(csi) {
                Some(tail) => rest = tail,
                None => {
                    kept.push('[');
                    rest = csi;
                }
            }
        } else if let Some(osc) = after.strip_prefix(']') {
            match osc_tail(osc) {
                Some(tail) => rest = tail,
                None => {
                    kept.push(']');
                    rest = osc;
                }
            }
        } else {
            kept.push('\x1b');
        }
    }
    kept.push_str(rest);
    clean(&kept)
}

/// `clean_ansi` line by line, keeping the breaks: a release body or
/// note keeps its paragraphs while sequences and controls are stripped.
pub fn clean_lines(text: &str) -> String {
    text.lines().map(clean_ansi).collect::<Vec<_>>().join("\n")
}

/// The text after a complete CSI sequence: `0x20–0x3F` parameter and
/// intermediate bytes, then one `0x40–0x7E` final byte. `None` when the
/// sequence runs out before its final byte.
fn csi_tail(s: &str) -> Option<&str> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && (0x20..=0x3f).contains(&b[i]) {
        i += 1;
    }
    match b.get(i) {
        Some(&final_byte) if (0x40..=0x7e).contains(&final_byte) => Some(&s[i + 1..]),
        _ => None,
    }
}

/// The text after a complete OSC sequence: payload until BEL or
/// `ESC \\` (ST). `None` when it never terminates.
fn osc_tail(s: &str) -> Option<&str> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == 0x07 {
            return Some(&s[i + 1..]);
        }
        if b[i] == 0x1b && b.get(i + 1) == Some(&b'\\') {
            return Some(&s[i + 2..]);
        }
        i += 1;
    }
    None
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
    fn ansi_sequences_drop_whole_and_controls_still_flatten() {
        assert_eq!(clean_ansi("\x1b[1;32mok\x1b[0m"), "ok");
        assert_eq!(clean_ansi("\x1b[2Jclear"), "clear");
        assert_eq!(clean_ansi("\x1b]52;c;AAAA\x07x"), "x");
        assert_eq!(clean_ansi("\x1b]8;;http://x\x1b\\y"), "y");
        assert_eq!(clean_ansi("a\x1b[1;3"), "a[1;3",);
        assert_eq!(clean_ansi("10%\r20%"), "10% 20%");
        assert_eq!(clean_ansi("tab\there"), "tab here");
        assert_eq!(clean_ansi("\x1bPq dcs text"), "Pq dcs text");
    }

    #[test]
    fn clean_lines_keeps_breaks_and_strips_sequences() {
        let body = "one\n- \x1b[31mred\x1b[0m\n- c1 \u{9b}x";
        assert_eq!(clean_lines(body), "one\n- red\n- c1 x");
        assert!(
            !clean_lines(body)
                .chars()
                .any(|c| c.is_control() && c != '\n')
        );
    }
}

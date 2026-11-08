// Derived from herdr src/terminal_notify.rs and src/terminal_effects.rs
// (Apache-2.0).
//! Outer-terminal notification sequences. The reference writes to stdout;
//! hc-tui returns the bytes so `App`'s `HostControlSink` (the real outer
//! terminal in production, a capture buffer in tests) owns the write.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalNotificationBackend {
    Ghostty,
    Iterm2,
    Kitty,
    WezTerm,
}

/// Inside tmux the sequence needs the `tmux;` passthrough wrapper.
pub fn in_tmux() -> bool {
    std::env::var_os("TMUX").is_some()
}

pub fn detect_backend() -> Option<TerminalNotificationBackend> {
    let term_program = std::env::var("TERM_PROGRAM").ok();
    let term = std::env::var("TERM").ok();

    match term_program.as_deref() {
        Some("ghostty") => return Some(TerminalNotificationBackend::Ghostty),
        Some("iTerm.app") => return Some(TerminalNotificationBackend::Iterm2),
        Some("WezTerm") => return Some(TerminalNotificationBackend::WezTerm),
        _ => {}
    }

    if std::env::var_os("KITTY_WINDOW_ID").is_some() {
        return Some(TerminalNotificationBackend::Kitty);
    }

    match term.as_deref() {
        Some("xterm-ghostty") => Some(TerminalNotificationBackend::Ghostty),
        Some("xterm-kitty") => Some(TerminalNotificationBackend::Kitty),
        Some(term) if term.contains("wezterm") => Some(TerminalNotificationBackend::WezTerm),
        _ => None,
    }
}

/// The reference `show_notification`: an OSC 9 or OSC 99 sequence for the
/// detected backend, tmux-passthrough wrapped inside tmux. `None` means
/// the host terminal advertises no notification backend.
pub fn notification_bytes(
    backend: Option<TerminalNotificationBackend>,
    in_tmux: bool,
    title: &str,
    body: Option<&str>,
) -> Option<Vec<u8>> {
    let sequence = match backend? {
        TerminalNotificationBackend::Ghostty
        | TerminalNotificationBackend::Iterm2
        | TerminalNotificationBackend::WezTerm => build_osc9_notification(title, body),
        TerminalNotificationBackend::Kitty => build_osc99_notification(title, body),
    };
    Some(if in_tmux {
        wrap_tmux_passthrough(&sequence)
    } else {
        sequence
    })
}

pub fn split_message(message: &str) -> (&str, Option<&str>) {
    match message.split_once(": ") {
        Some((title, body)) if !title.is_empty() && !body.is_empty() => (title, Some(body)),
        _ => (message, None),
    }
}

fn build_osc9_notification(title: &str, body: Option<&str>) -> Vec<u8> {
    let message = sanitize_text(match body {
        Some(body) if !body.is_empty() => format!("{title}: {body}"),
        _ => title.to_string(),
    });
    format!("\x1b]9;{message}\x1b\\").into_bytes()
}

fn build_osc99_notification(title: &str, body: Option<&str>) -> Vec<u8> {
    let title = sanitize_text(title);
    match body {
        Some(body) if !body.is_empty() => {
            let body = sanitize_text(body);
            format!("\x1b]99;i=1:d=0;{title}\x1b\\\x1b]99;i=1:p=body;{body}\x1b\\").into_bytes()
        }
        _ => format!("\x1b]99;;{title}\x1b\\").into_bytes(),
    }
}

fn sanitize_text(text: impl AsRef<str>) -> String {
    text.as_ref()
        .chars()
        .filter(|ch| *ch != '\u{1b}' && *ch != '\u{7}' && *ch != '\u{9c}')
        .map(|ch| match ch {
            '\n' | '\r' | '\t' => ' ',
            _ => ch,
        })
        .collect()
}

fn wrap_tmux_passthrough(sequence: &[u8]) -> Vec<u8> {
    let mut wrapped = Vec::with_capacity(sequence.len() + 16);
    wrapped.extend_from_slice(b"\x1bPtmux;");
    for &byte in sequence {
        if byte == 0x1b {
            wrapped.push(0x1b);
        }
        wrapped.push(byte);
    }
    wrapped.extend_from_slice(b"\x1b\\");
    wrapped
}

const BELL_CHUNK: [u8; 64] = [b'\x07'; 64];

/// The reference `write_terminal_bells`: BEL bytes in 64-byte chunks.
/// Returns the bytes for the caller's sink (u64 count — pane BEL counts
/// can exceed u16 under a flood).
pub fn terminal_bells(count: u64) -> Vec<u8> {
    let full_chunks = usize::try_from(count / BELL_CHUNK.len() as u64).unwrap_or(usize::MAX);
    let remainder = usize::try_from(count % BELL_CHUNK.len() as u64).unwrap_or(BELL_CHUNK.len());
    let mut bytes = Vec::with_capacity(
        full_chunks
            .saturating_mul(BELL_CHUNK.len())
            .saturating_add(remainder),
    );
    for _ in 0..full_chunks {
        bytes.extend_from_slice(&BELL_CHUNK);
    }
    bytes.extend_from_slice(&BELL_CHUNK[..remainder.min(BELL_CHUNK.len())]);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_message_splits_title_and_body() {
        assert_eq!(
            split_message("agent done: ws · 1"),
            ("agent done", Some("ws · 1"))
        );
    }

    #[test]
    fn split_message_leaves_plain_message_alone() {
        assert_eq!(split_message("agent done"), ("agent done", None));
    }

    #[test]
    fn sanitize_text_strips_control_bytes() {
        assert_eq!(sanitize_text("a\n\tb\u{1b}c\u{7}"), "a  bc");
    }

    #[test]
    fn kitty_notification_uses_structured_title_and_body() {
        let sequence = String::from_utf8(build_osc99_notification("pi finished", Some("ws · 1")))
            .expect("utf8");
        assert!(sequence.contains("]99;i=1:d=0;pi finished"));
        assert!(sequence.contains("]99;i=1:p=body;ws · 1"));
    }

    #[test]
    fn tmux_passthrough_wraps_and_escapes() {
        let wrapped = wrap_tmux_passthrough(b"\x1b]9;hi\x1b\\");
        assert_eq!(wrapped, b"\x1bPtmux;\x1b\x1b]9;hi\x1b\x1b\\\x1b\\");
    }

    #[test]
    fn no_backend_produces_no_bytes() {
        assert!(notification_bytes(None, false, "t", None).is_none());
    }

    #[test]
    fn bells_write_the_exact_count() {
        assert_eq!(terminal_bells(130), vec![b'\x07'; 130]);
        assert!(terminal_bells(0).is_empty());
    }
}

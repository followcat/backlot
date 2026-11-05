// Derived from herdr src/raw_input.rs (Apache-2.0).
//! Byte-level sequence helpers shared by the framer and the event
//! classifier: control-string detection, CSI finals, UTF-8 width,
//! bracketed-paste markers, and the SGR/default-mouse report shapes.
//! All pure — no I/O, no timing policy.

use hc_vt::Mods;

use crate::app::input::{MouseButton, MouseEventKind, MouseInput};

pub(crate) const ESC: u8 = 0x1b;
pub(crate) const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
pub(crate) const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";
pub(crate) const GHOSTTY_COLOR_SCHEME_DARK_REPORT: &[u8] = b"\x1b[?997;1n";
pub(crate) const GHOSTTY_COLOR_SCHEME_LIGHT_REPORT: &[u8] = b"\x1b[?997;2n";
pub(crate) const MAX_DISCARDED_CONTROL_TAIL_BYTES: usize = 128;
pub(crate) const MAX_ORPHANED_SGR_MOUSE_TAIL_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlStringFamily {
    Osc,
    StTerminated,
    HostReplyCsi,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlString {
    Complete {
        len: usize,
        family: ControlStringFamily,
    },
    Incomplete {
        family: ControlStringFamily,
    },
}

pub(crate) fn control_string(buffer: &[u8]) -> Option<ControlString> {
    let family = match buffer.get(..2)? {
        b"\x1b]" => ControlStringFamily::Osc,
        b"\x1bP" | b"\x1b_" | b"\x1b^" | b"\x1bX" => ControlStringFamily::StTerminated,
        _ => return None,
    };

    Some(match control_string_terminator_for_family(buffer, family) {
        Some(len) => ControlString::Complete { len, family },
        None => ControlString::Incomplete { family },
    })
}

pub(crate) fn osc_string_terminator(buffer: &[u8]) -> Option<usize> {
    let st = find_subsequence(buffer, b"\x1b\\").map(|idx| idx + 2);
    let bel = buffer
        .iter()
        .position(|byte| *byte == b'\x07')
        .map(|idx| idx + 1);
    match (st, bel) {
        (Some(st), Some(bel)) => Some(st.min(bel)),
        (Some(st), None) => Some(st),
        (None, Some(bel)) => Some(bel),
        (None, None) => None,
    }
}

fn st_string_terminator(buffer: &[u8]) -> Option<usize> {
    find_subsequence(buffer, b"\x1b\\").map(|idx| idx + 2)
}

pub(crate) fn control_string_terminator_for_family(
    buffer: &[u8],
    family: ControlStringFamily,
) -> Option<usize> {
    match family {
        ControlStringFamily::Osc => osc_string_terminator(buffer),
        ControlStringFamily::StTerminated => st_string_terminator(buffer),
        ControlStringFamily::HostReplyCsi => None,
    }
}

pub(crate) fn find_csi_final(buffer: &[u8], finals: &[u8]) -> Option<usize> {
    for (idx, byte) in buffer.iter().enumerate().skip(2) {
        if finals.contains(byte) {
            return Some(idx + 1);
        }
    }
    None
}

pub(crate) fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Length of the complete escape sequence at `buffer`'s head —
/// `None` while the tail is still arriving.
pub(crate) fn complete_escape_sequence_len(buffer: &[u8]) -> Option<usize> {
    if buffer.len() == 1 {
        return None;
    }

    if buffer.starts_with(b"\x1b\x1b[<")
        && let Some(mouse_len) = find_csi_final(&buffer[1..], b"Mm")
        && std::str::from_utf8(&buffer[1..1 + mouse_len])
            .ok()
            .and_then(parse_sgr_mouse)
            .is_some()
    {
        return Some(1);
    }

    if buffer.len() >= 7
        && buffer.starts_with(b"\x1b\x1b[M")
        && parse_default_mouse(&buffer[1..7]).is_some()
    {
        return Some(1);
    }

    if buffer.starts_with(b"\x1b\x1b") {
        return complete_escape_sequence_len(&buffer[1..]).map(|len| len + 1);
    }

    if buffer.starts_with(b"\x1b[") {
        if buffer.starts_with(b"\x1b[<") {
            return find_csi_final(buffer, b"Mm");
        }
        if buffer.starts_with(b"\x1b[M") {
            return (buffer.len() >= 6).then_some(6);
        }
        return find_csi_final(
            buffer,
            b"@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~",
        );
    }

    if let Some(control) = control_string(buffer) {
        return match control {
            ControlString::Complete { len, .. } => Some(len),
            ControlString::Incomplete { .. } => None,
        };
    }

    if buffer.starts_with(b"\x1bO") {
        return (buffer.len() >= 3).then_some(3);
    }

    let escaped_char_width = utf8_char_width(buffer[1])?;
    if buffer.len() < 1 + escaped_char_width {
        return None;
    }
    std::str::from_utf8(&buffer[1..1 + escaped_char_width]).ok()?;
    Some(1 + escaped_char_width)
}

pub(crate) fn first_complete_utf8_char_len(buffer: &[u8]) -> Option<usize> {
    let width = utf8_char_width(*buffer.first()?)?;

    if buffer.len() < width {
        return None;
    }

    std::str::from_utf8(&buffer[..width]).ok()?;
    Some(width)
}

pub(crate) fn starts_with_incomplete_utf8_char(buffer: &[u8]) -> bool {
    match std::str::from_utf8(buffer) {
        Ok(_) => false,
        Err(err) => err.valid_up_to() == 0 && err.error_len().is_none(),
    }
}

pub(crate) fn utf8_char_width(first: u8) -> Option<usize> {
    if first < 0x80 {
        Some(1)
    } else if first & 0b1110_0000 == 0b1100_0000 {
        Some(2)
    } else if first & 0b1111_0000 == 0b1110_0000 {
        Some(3)
    } else if first & 0b1111_1000 == 0b1111_0000 {
        Some(4)
    } else {
        None
    }
}

/// Whether bytes start with a complete sequence shaped like a kitty
/// key report, `CSI <code>[:alternates][;mods[:event][;text]] u`.
pub(crate) fn starts_with_kitty_key_report(bytes: &[u8]) -> bool {
    let Some(rest) = bytes.strip_prefix(b"\x1b[") else {
        return false;
    };
    let params = rest
        .iter()
        .take_while(|byte| byte.is_ascii_digit() || matches!(byte, b';' | b':'))
        .count();
    rest.first().is_some_and(u8::is_ascii_digit) && rest.get(params) == Some(&b'u')
}

/// ESC plus a byte that can also start a longer sequence: Alt+[,
/// Alt+O, and Alt+Escape (ESC ESC, kept whole where doubled escapes
/// are preserved).
pub(crate) fn is_alt_key_introducer(buffer: &[u8]) -> bool {
    matches!(buffer, [ESC, b'[' | b'O' | ESC])
}

/// Pending bytes that could still be a key on their own: a lone ESC
/// or ESC plus one introducer.
pub(crate) fn is_escape_key_prefix(buffer: &[u8]) -> bool {
    match buffer {
        [ESC] => true,
        [ESC, introducer] => matches!(
            introducer,
            b'[' | b'O' | b'P' | b']' | b'X' | b'^' | b'_' | 0x1b
        ),
        _ => false,
    }
}

pub(crate) fn starts_with_incomplete_default_color_response(buffer: &[u8]) -> bool {
    // The prefix check keeps this constant-cost for ordinary input.
    matches!(buffer.get(..5), Some(b"\x1b]10;" | b"\x1b]11;"))
        && matches!(
            control_string(buffer),
            Some(ControlString::Incomplete {
                family: ControlStringFamily::Osc
            })
        )
}

pub(crate) fn starts_with_incomplete_host_color_scheme_report(buffer: &[u8]) -> bool {
    buffer.starts_with(b"\x1b[?")
        && (GHOSTTY_COLOR_SCHEME_DARK_REPORT.starts_with(buffer)
            || GHOSTTY_COLOR_SCHEME_LIGHT_REPORT.starts_with(buffer))
        && buffer.len() < GHOSTTY_COLOR_SCHEME_DARK_REPORT.len()
}

/// `CSI 6 ; h ; w` in progress — an XTWINOPS cell-size report tail.
pub(crate) fn starts_with_incomplete_host_cell_size_report(buffer: &[u8]) -> bool {
    let Some(body) = buffer.strip_prefix(b"\x1b[") else {
        return false;
    };
    if body.is_empty() || body.last() == Some(&b't') {
        return false;
    }

    let mut params = body.split(|byte| *byte == b';');
    if params.next() != Some(b"6".as_slice()) {
        return false;
    }
    let height = params.next();
    let width = params.next();
    params.next().is_none()
        && height.is_none_or(|value| value.iter().all(u8::is_ascii_digit))
        && width.is_none_or(|value| value.iter().all(u8::is_ascii_digit))
        && !(height.is_some_and(<[u8]>::is_empty) && width.is_some())
}

pub(crate) fn starts_with_incomplete_sgr_mouse_sequence(buffer: &[u8]) -> bool {
    buffer.starts_with(b"\x1b[<")
        && buffer[3..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || *byte == b';')
}

/// Whether bytes after a held ESC or ESC[ still read as one escape
/// sequence, such as a mouse report, host reply, or bracketed paste,
/// rather than a key.
pub(crate) fn continues_escape_sequence(buffer: &[u8]) -> bool {
    match buffer {
        [ESC] | [ESC, b'['] => true,
        // OSC, DCS and APC host replies can also split after their ESC.
        [ESC, b']' | b'P' | b'_', ..] => true,
        [ESC, b'[', next, ..] => *next == b'M' || matches!(*next, 0x20..=0x3f),
        _ => false,
    }
}

pub(crate) fn could_continue_as_mouse_report(buffer: &[u8]) -> bool {
    [b"\x1b[<".as_slice(), b"\x1b[M"]
        .iter()
        .any(|prefix| buffer.starts_with(prefix) || prefix.starts_with(buffer))
}

pub(crate) fn starts_with_incomplete_orphaned_sgr_mouse_tail(buffer: &[u8]) -> bool {
    if buffer.len() > MAX_ORPHANED_SGR_MOUSE_TAIL_BYTES {
        return false;
    }
    buffer.len() < 3 && b"[<".starts_with(buffer)
        || buffer.starts_with(b"[<")
            && buffer[2..]
                .iter()
                .all(|byte| byte.is_ascii_digit() || *byte == b';')
}

pub(crate) fn discard_complete_orphaned_sgr_mouse_tail(buffer: &mut Vec<u8>) -> bool {
    let Some(terminator_len) = buffer
        .iter()
        .position(|byte| matches!(*byte, b'M' | b'm'))
        .map(|idx| idx + 1)
    else {
        return false;
    };
    if terminator_len > MAX_ORPHANED_SGR_MOUSE_TAIL_BYTES {
        return false;
    }
    let mut sequence = Vec::with_capacity(terminator_len + 1);
    sequence.push(ESC);
    sequence.extend_from_slice(&buffer[..terminator_len]);
    let Ok(sequence) = std::str::from_utf8(&sequence) else {
        return false;
    };
    if parse_sgr_mouse(sequence).is_none() {
        return false;
    }
    buffer.drain(..terminator_len);
    true
}

pub(crate) fn discard_host_reply_csi_tail(
    buffer: &mut Vec<u8>,
    discarded_tail_bytes: &mut usize,
) -> bool {
    let remaining = MAX_DISCARDED_CONTROL_TAIL_BYTES.saturating_sub(*discarded_tail_bytes);
    let inspected = buffer.len().min(remaining);

    for index in 0..inspected {
        match buffer[index] {
            0x20..=0x3f => {}
            0x40..=0x7e => {
                buffer.drain(..=index);
                return true;
            }
            _ => {
                buffer.drain(..index);
                return true;
            }
        }
    }

    buffer.drain(..inspected);
    *discarded_tail_bytes = discarded_tail_bytes.saturating_add(inspected);
    *discarded_tail_bytes >= MAX_DISCARDED_CONTROL_TAIL_BYTES
}

#[path = "raw_frames_mouse.rs"]
mod mouse;
pub(crate) use mouse::*;

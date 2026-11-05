// Derived from herdr src/raw_input.rs (Apache-2.0).
use super::*;

pub(crate) enum SgrMouseContinuation {
    Incomplete,
    Complete(usize),
    Invalid,
}

pub(crate) fn classify_sgr_mouse_continuation(prefix: &[u8], tail: &[u8]) -> SgrMouseContinuation {
    let remaining = MAX_DISCARDED_CONTROL_TAIL_BYTES.saturating_sub(prefix.len());
    let tail = &tail[..tail.len().min(remaining)];
    let final_index = tail
        .iter()
        .position(|byte| !byte.is_ascii_digit() && *byte != b';');
    let payload_len = final_index.unwrap_or(tail.len());
    let mut report = prefix.to_vec();
    report.extend_from_slice(&tail[..payload_len]);
    if !plausible_sgr_mouse_prefix(&report) {
        return SgrMouseContinuation::Invalid;
    }
    if let Some(index) = final_index {
        report.push(tail[index]);
        let valid = std::str::from_utf8(&report)
            .ok()
            .and_then(parse_sgr_mouse)
            .is_some();
        return if valid {
            SgrMouseContinuation::Complete(index + 1)
        } else {
            SgrMouseContinuation::Invalid
        };
    }
    SgrMouseContinuation::Incomplete
}

/// Shape check only: `ESC [ <` then `Cb ; x ; y` fields that could
/// still complete a valid SGR report.
pub(crate) fn plausible_sgr_mouse_prefix(report: &[u8]) -> bool {
    let Some(body) = report.strip_prefix(b"\x1b[<") else {
        return false;
    };
    let mut fields = body.split(|byte| *byte == b';').peekable();
    for field in 0..3 {
        let Some(digits) = fields.next() else {
            return fields.peek().is_none();
        };
        if digits.is_empty() {
            return fields.peek().is_none();
        }
        if !digits.iter().all(u8::is_ascii_digit) {
            return false;
        }
        let Some(value) = std::str::from_utf8(digits)
            .ok()
            .and_then(|digits| digits.parse::<u16>().ok())
        else {
            return false;
        };
        if field == 0 && value > u16::from(u8::MAX) {
            return false;
        }
        if fields.peek().is_some()
            && ((field == 0 && parse_mouse_cb(value as u8).is_none()) || (field == 1 && value == 0))
        {
            return false;
        }
    }
    true
}

pub(crate) fn plausible_control_string_tail(family: ControlStringFamily, buffer: &[u8]) -> bool {
    match family {
        ControlStringFamily::Osc => buffer.iter().all(|byte| {
            byte.is_ascii_digit()
                || matches!(
                    *byte,
                    b';' | b':'
                        | b'/'
                        | b'#'
                        | b'?'
                        | b'.'
                        | b'_'
                        | b'-'
                        | b'+'
                        | b'r'
                        | b'g'
                        | b'b'
                        | b'R'
                        | b'G'
                        | b'B'
                        | ESC
                )
        }),
        ControlStringFamily::StTerminated => buffer.last() == Some(&ESC),
        ControlStringFamily::HostReplyCsi => false,
    }
}

/// One SGR mouse report (`CSI < Cb ; x ; y M/m`) with raw 1-based
/// coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SgrMouseReport {
    pub(crate) kind: MouseEventKind,
    pub(crate) modifiers: Mods,
    pub(crate) x: u32,
    pub(crate) y: u32,
}

pub(crate) fn parse_sgr_mouse_report(sequence: &[u8]) -> Option<SgrMouseReport> {
    let body = sequence.strip_prefix(b"\x1b[<")?;
    let (&final_byte, payload) = body.split_last()?;
    if final_byte != b'M' && final_byte != b'm' {
        return None;
    }
    let mut parts = payload.split(|byte| *byte == b';');
    let cb = u8::try_from(parse_report_number(parts.next()?)?).ok()?;
    let x = parse_report_number(parts.next()?)?;
    let y = parse_report_number(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    let (kind, modifiers) = parse_mouse_cb(cb)?;
    let kind = match (final_byte, kind) {
        (b'm', MouseEventKind::Down(button)) => MouseEventKind::Up(button),
        (_, kind) => kind,
    };
    Some(SgrMouseReport {
        kind,
        modifiers,
        x,
        y,
    })
}

pub(crate) fn parse_sgr_mouse(sequence: &str) -> Option<MouseInput> {
    let report = parse_sgr_mouse_report(sequence.as_bytes())?;
    let col = u16::try_from(report.x.checked_sub(1)?).unwrap_or(u16::MAX);
    let row = u16::try_from(report.y.checked_sub(1)?).unwrap_or(u16::MAX);
    Some(MouseInput {
        kind: report.kind,
        col,
        row,
        mods: report.modifiers,
        sgr_position: Some((report.x, report.y)),
    })
}

pub(crate) fn parse_default_mouse(sequence: &[u8]) -> Option<MouseInput> {
    let &[ESC, b'[', b'M', encoded_cb, encoded_column, encoded_row] = sequence else {
        return None;
    };
    let cb = encoded_cb.checked_sub(32)?;
    let col = u16::from(encoded_column).checked_sub(33)?;
    let row = u16::from(encoded_row).checked_sub(33)?;
    let (kind, mods) = parse_mouse_cb(cb)?;
    Some(MouseInput {
        kind,
        col,
        row,
        mods,
        sgr_position: None,
    })
}

pub(super) fn parse_report_number(value: &[u8]) -> Option<u32> {
    (!value.is_empty() && value.iter().all(u8::is_ascii_digit))
        .then(|| std::str::from_utf8(value).ok()?.parse().ok())
        .flatten()
}

pub(super) fn parse_mouse_cb(cb: u8) -> Option<(MouseEventKind, Mods)> {
    let button_number = (cb & 0b0000_0011) | ((cb & 0b1100_0000) >> 4);
    let dragging = cb & 0b0010_0000 == 0b0010_0000;
    let kind = match (button_number, dragging) {
        (0, false) => MouseEventKind::Down(MouseButton::Left),
        (1, false) => MouseEventKind::Down(MouseButton::Middle),
        (2, false) => MouseEventKind::Down(MouseButton::Right),
        (0, true) => MouseEventKind::Drag(MouseButton::Left),
        (1, true) => MouseEventKind::Drag(MouseButton::Middle),
        (2, true) => MouseEventKind::Drag(MouseButton::Right),
        (3, false) => MouseEventKind::Up(MouseButton::Left),
        // Preserve extended-button drags as motion so a stuck host
        // button cannot suppress hover.
        (3, true) | (4, true) | (5, true) | (8, true) | (9, true) => MouseEventKind::Moved,
        (4, false) => MouseEventKind::ScrollUp,
        (5, false) => MouseEventKind::ScrollDown,
        (6, false) => MouseEventKind::ScrollLeft,
        (7, false) => MouseEventKind::ScrollRight,
        _ => return None,
    };
    let mut modifiers = Mods::NONE;
    if cb & 0b0000_0100 != 0 {
        modifiers = modifiers.union(Mods::SHIFT);
    }
    if cb & 0b0000_1000 != 0 {
        modifiers = modifiers.union(Mods::ALT);
    }
    if cb & 0b0001_0000 != 0 {
        modifiers = modifiers.union(Mods::CTRL);
    }
    Some((kind, modifiers))
}

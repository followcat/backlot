//! Bounded framing retains only a delimiter prefix while discarding an
//! oversized record. Its body can never be reinterpreted as commands.

use super::*;
use crate::app::input::MAX_INPUT_BYTES;

#[derive(Default)]
pub(super) struct InputLimits {
    discard: Option<Discard>,
    rejection: Option<&'static str>,
}

enum Discard {
    Paste(usize),
    Control {
        family: ControlStringFamily,
        escape: bool,
    },
    Csi,
}

impl InputLimits {
    pub(super) fn consume<'a>(&mut self, bytes: &'a [u8]) -> &'a [u8] {
        if self.discard.is_none() {
            return bytes;
        }
        for (index, &byte) in bytes.iter().enumerate() {
            let done = match self.discard.as_mut().expect("discard state") {
                Discard::Paste(matched) => {
                    *matched = if byte == BRACKETED_PASTE_END[*matched] {
                        *matched + 1
                    } else {
                        usize::from(byte == ESC)
                    };
                    *matched == BRACKETED_PASTE_END.len()
                }
                Discard::Control { family, escape } => {
                    let done = (*escape && byte == b'\\')
                        || (*family == ControlStringFamily::Osc && byte == 7);
                    *escape = byte == ESC;
                    done
                }
                Discard::Csi => (0x40..=0x7e).contains(&byte),
            };
            if done {
                self.discard = None;
                return &bytes[index + 1..];
            }
        }
        &[]
    }
}

impl RawInputByteFramer {
    pub(super) fn enforce_limits(&mut self) {
        let (discard, skip, rejection) = if self.buffer.starts_with(BRACKETED_PASTE_START) {
            let end =
                find_subsequence(&self.buffer, BRACKETED_PASTE_END).unwrap_or(self.buffer.len());
            if end.saturating_sub(BRACKETED_PASTE_START.len()) <= MAX_INPUT_BYTES {
                return;
            }
            (
                Discard::Paste(0),
                BRACKETED_PASTE_START.len(),
                Some("Paste exceeds the 256 KiB input limit"),
            )
        } else if self.buffer.len() > MAX_INPUT_BYTES {
            if let Some(family) = self.discard_until.take() {
                let discard = if family == ControlStringFamily::HostReplyCsi {
                    Discard::Csi
                } else {
                    Discard::Control {
                        family,
                        escape: false,
                    }
                };
                (discard, 0, None)
            } else if let Some(control) = control_string(&self.buffer) {
                let family = match control {
                    ControlString::Complete { family, .. }
                    | ControlString::Incomplete { family } => family,
                };
                (
                    Discard::Control {
                        family,
                        escape: false,
                    },
                    2,
                    None,
                )
            } else if self.buffer.starts_with(b"\x1b[") {
                (
                    Discard::Csi,
                    2,
                    Some("Key report exceeds the 256 KiB input limit"),
                )
            } else {
                return;
            }
        } else {
            return;
        };
        self.limits.discard = Some(discard);
        if rejection.is_some() {
            self.limits.rejection = rejection;
        }
        let oversized = std::mem::take(&mut self.buffer);
        self.buffer
            .extend_from_slice(self.limits.consume(&oversized[skip..]));
    }

    pub(crate) fn take_rejection(&mut self) -> Option<&'static str> {
        self.limits.rejection.take()
    }

    /// Paste and partial UTF-8 cannot resolve on a timer. Waiting for the
    /// next read avoids periodic wakeups while preserving framing behavior.
    pub(crate) fn needs_timeout(&self) -> bool {
        let incomplete_paste = self.buffer.starts_with(BRACKETED_PASTE_START)
            && find_subsequence(&self.buffer, BRACKETED_PASTE_END).is_none();
        let incomplete_text = starts_with_incomplete_utf8_char(&self.buffer)
            || (self.buffer.first() == Some(&ESC)
                && starts_with_incomplete_utf8_char(&self.buffer[1..]));
        self.has_pending_input() && !incomplete_paste && !incomplete_text
    }

    /// EOF resolves a complete trailing Escape/Alt introducer, but never
    /// promotes incomplete paste, UTF-8 or control payloads to typed keys.
    pub(crate) fn finish(&mut self) -> Vec<Vec<u8>> {
        self.host_color_replies_awaited = 0;
        self.host_cell_size_replies_awaited = 0;
        self.host_appearance_reply_awaited = false;
        self.host_reply_activity_at = None;
        self.host_escape_disambiguation_active = false;
        self.awaiting_mouse_tail_after = None;
        self.flush_timeout()
    }
}

#[cfg(test)]
mod tests;

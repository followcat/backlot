// Derived from herdr src/raw_input.rs (Apache-2.0).
//! Timeout resolution, kept separate from stream framing.

use super::*;

impl RawInputByteFramer {
    /// What held bytes finally are, once the stream stayed idle one
    /// flush window: complete sequences emit, incomplete tails the
    /// host could still send wait one more cycle, and the rest are
    /// either keys (lone ESC, Alt introducers) or dropped garbage.
    pub(crate) fn flush_timeout(&mut self) -> Vec<Vec<u8>> {
        let mut chunks = self.drain_available_chunks();
        self.expire_unanswered_host_queries();

        // Idle is not evidence that a mouse report has ended. The
        // continuation stays bounded and is released if it cannot
        // complete a valid report.
        if self.timed_out_mouse_prefix.is_some() {
            return chunks;
        }

        if let Some(family) = self.discard_until {
            if family == ControlStringFamily::HostReplyCsi {
                return chunks;
            }
            let keep_split_st = self.buffer.last() == Some(&ESC);
            let keep_discarding = plausible_control_string_tail(family, &self.buffer);
            self.discarded_tail_bytes = self.discarded_tail_bytes.saturating_add(self.buffer.len());
            self.buffer.clear();
            if keep_discarding && self.discarded_tail_bytes <= MAX_DISCARDED_CONTROL_TAIL_BYTES {
                if keep_split_st {
                    self.buffer.push(ESC);
                }
            } else {
                self.discard_until = None;
                self.discarded_tail_bytes = 0;
            }
            return chunks;
        }

        if self.buffer.is_empty() {
            return chunks;
        }

        // A confirmed host sends Escape as `CSI 27u`, so a mouse report
        // prefix that outlives keyboard timing may still get a delayed
        // tail (#3480). Keep it briefly; `push` releases it if other
        // input follows.
        // A finished mouse wait also counts as this prefix's one-flush
        // host reply hold, so the two holds never stack.
        let mouse_wait_served = self.awaiting_mouse_tail_after.take().is_some();
        // Prefixes already inside a sequence waited the long window;
        // only the ambiguous ESC / ESC[ prefixes need this extra
        // mouse-tail hold.
        if !mouse_wait_served
            && self.host_escape_disambiguation_active
            && self.pending_could_be_escape_key()
            && could_continue_as_mouse_report(&self.buffer)
        {
            self.awaiting_mouse_tail_after = Some(self.buffer.len());
            return chunks;
        }

        if self.lone_escape_recently_flushed && self.buffer.starts_with(b"[<") {
            let mut prefix = vec![ESC];
            prefix.append(&mut self.buffer);
            self.retain_timed_out_mouse_prefix(prefix);
            self.lone_escape_recently_flushed = false;
            return chunks;
        }

        // An unfinished bracketed paste is paste content, never keys —
        // it waits for `201~` however long that takes.
        if self.buffer.starts_with(BRACKETED_PASTE_START)
            && find_subsequence(&self.buffer, BRACKETED_PASTE_END).is_none()
        {
            return chunks;
        }

        if starts_with_incomplete_sgr_mouse_sequence(&self.buffer) && !self.awaiting_host_reply() {
            if !self.held_pending_host_reply_esc && !mouse_wait_served {
                self.held_pending_host_reply_esc = true;
                return chunks;
            }
            // A second idle flush with no tail: the report is dead.
            self.retain_timed_out_mouse_prefix(self.buffer.clone());
            self.buffer.clear();
            self.held_pending_host_reply_esc = false;
            return chunks;
        }

        if starts_with_incomplete_default_color_response(&self.buffer) {
            if self.host_color_reply_abandoned() {
                // The host went quiet mid-reply; never let it hold input.
                self.buffer.clear();
                self.partial_host_color_reply_read_at = None;
                return chunks;
            }
            return chunks;
        }

        if (self.host_cell_size_replies_awaited > 0 || self.host_appearance_reply_awaited)
            && self.buffer.as_slice() == b"\x1b["
        {
            if !self.held_pending_host_reply_esc && !mouse_wait_served {
                self.held_pending_host_reply_esc = true;
                return chunks;
            }
            self.host_cell_size_replies_awaited = 0;
            self.host_appearance_reply_awaited = false;
            self.held_pending_host_reply_esc = false;
        }

        if self.host_cell_size_replies_awaited > 0
            && starts_with_incomplete_host_cell_size_report(&self.buffer)
        {
            self.host_cell_size_replies_awaited = 0;
            self.held_pending_host_reply_esc = false;
            self.discard_until = Some(ControlStringFamily::HostReplyCsi);
            self.discarded_tail_bytes = 0;
            self.buffer.clear();
            return chunks;
        }

        if starts_with_incomplete_host_color_scheme_report(&self.buffer) {
            if self.host_appearance_reply_awaited && !self.held_pending_host_reply_esc {
                self.held_pending_host_reply_esc = true;
                return chunks;
            }
            self.host_appearance_reply_awaited = false;
            self.held_pending_host_reply_esc = false;
            self.discard_until = Some(ControlStringFamily::HostReplyCsi);
            self.discarded_tail_bytes = 0;
            self.buffer.clear();
            return chunks;
        }

        // A legacy host sends Alt+[, Alt+O and Alt+Escape as ESC plus
        // that byte. When nothing followed and no reply is expected,
        // it was the key. String introducers (OSC, DCS, APC, PM, SOS)
        // also start host replies, so they keep the control-string path
        // and are never forwarded as Alt keys (#344).
        if is_alt_key_introducer(&self.buffer) && !self.awaiting_host_reply() {
            chunks.push(std::mem::take(&mut self.buffer));
            return chunks;
        }

        if let Some(ControlString::Incomplete { family }) = control_string(&self.buffer) {
            // Host control replies take precedence over legacy Alt
            // forms like Alt+] after timeout, so later reply tails
            // cannot leak.
            self.discard_until = Some(family);
            self.discarded_tail_bytes = 0;
            self.buffer.clear();
            return chunks;
        }

        if self.buffer.as_slice() == [ESC] {
            if self.awaiting_host_reply() && !self.held_pending_host_reply_esc && !mouse_wait_served
            {
                self.held_pending_host_reply_esc = true;
                return chunks;
            }
            // No continuation arrived; give up the window so Escape is
            // not delayed again.
            self.host_color_replies_awaited = 0;
            self.host_cell_size_replies_awaited = 0;
            self.host_appearance_reply_awaited = false;
            self.held_pending_host_reply_esc = false;
            self.lone_escape_recently_flushed = true;
            chunks.push(std::mem::take(&mut self.buffer));
            return chunks;
        }

        if let Ok(text) = std::str::from_utf8(&self.buffer)
            && crate::raw_events::parse_terminal_key_sequence(text).is_some()
        {
            chunks.push(std::mem::take(&mut self.buffer));
            return chunks;
        }

        if starts_with_incomplete_utf8_char(&self.buffer) {
            return chunks;
        }

        if self.buffer.first() == Some(&ESC) && starts_with_incomplete_utf8_char(&self.buffer[1..])
        {
            return chunks;
        }

        self.lone_escape_recently_flushed = false;
        self.buffer.clear();
        chunks
    }
}

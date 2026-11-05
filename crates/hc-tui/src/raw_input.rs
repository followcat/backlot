// Derived from herdr src/raw_input.rs (Apache-2.0).
//! The reference `RawInputByteFramer` (unix surface): one owner for the
//! input byte stream that splits reads into complete input chunks and
//! answers a two-stage idle policy — `idle_flush_timeout_ms` decides
//! how long ambiguous prefixes may wait, `flush_timeout` decides what
//! held bytes finally are. Host-terminal replies (OSC colors, cell
//! size, appearance) share the stream with user input, so the framer
//! tracks outstanding queries and holds short ambiguous prefixes only
//! while a reply is plausibly in flight — never longer.
//!
//! User keys and bracketed paste typed while replies stream in are
//! preserved verbatim; replies split across reads stitch back together;
//! an unfinished reply that outlives its idle limit is dropped rather
//! than holding real input hostage.

use std::time::{Duration, Instant};

use crate::raw_events::Decoded;
use crate::raw_frames::*;

mod flush;
mod limits;

/// How long ambiguous keyboard bytes may wait for a tail.
pub(crate) const RAW_INPUT_IDLE_FLUSH_TIMEOUT_MS: i32 = 10;
/// Covers the 33 ms split in #4630 without gluing legacy Alt+[ to the
/// next key.
pub(crate) const MOUSE_ACTIVE_CSI_INTRODUCER_FLUSH_TIMEOUT_MS: i32 = 50;
/// A lone ESC under mouse capture gets a longer window — SGR reports
/// split after their introducer over slow links.
pub(crate) const MOUSE_ACTIVE_ESCAPE_SEQUENCE_FLUSH_TIMEOUT_MS: i32 = 150;
/// Covers the 350 ms mouse tail delay in #3480. Other input ends it
/// early (#4751).
const DISAMBIGUATED_MOUSE_TAIL_FLUSH_TIMEOUT_MS: i32 = 500;
/// Bytes already inside a sequence cannot be a key, so wait long
/// enough for a split read over a slow link before giving up on them.
pub(crate) const INCOMPLETE_SEQUENCE_FLUSH_TIMEOUT_MS: i32 = 500;

const HOST_COLOR_QUERY_REPLIES: u16 = 258;
/// How long after the last query or reply the framer still treats
/// `ESC[` / `ESC]` as a possible host reply instead of a key.
const HOST_REPLY_QUIET_WINDOW: Duration = Duration::from_secs(1);
/// How long an unfinished OSC 10/11 reply may wait for its next read
/// before it is dropped (#5052). Twice the split-read wait for any
/// incomplete sequence, and far above the tens of milliseconds seen
/// between fragments over SSH and tmux, yet short enough that a reply
/// the link cut off cannot hold input for long.
const HOST_COLOR_REPLY_IDLE_LIMIT: Duration = Duration::from_secs(1);
const HOST_CELL_SIZE_QUERY_REPLIES: u16 = 1;

/// The byte framer — `push` reads split into per-event chunks,
/// `flush_timeout` resolves what an idle tail was.
#[derive(Default)]
pub(crate) struct RawInputByteFramer {
    limits: limits::InputLimits,
    buffer: Vec<u8>,
    discard_until: Option<ControlStringFamily>,
    discarded_tail_bytes: usize,
    // The discarded prefix is kept separate from continuation bytes
    // awaiting validation.
    timed_out_mouse_prefix: Option<Vec<u8>>,
    lone_escape_recently_flushed: bool,
    host_color_replies_awaited: u16,
    host_cell_size_replies_awaited: u16,
    host_appearance_reply_awaited: bool,
    /// Last query sent or reply received. Hosts may answer only part
    /// of a query (or none), so the reply counters alone never expire.
    host_reply_activity_at: Option<Instant>,
    /// Last read that left an unfinished OSC 10/11 reply buffered.
    partial_host_color_reply_read_at: Option<Instant>,
    held_pending_host_reply_esc: bool,
    host_color_scheme_change_tracking: bool,
    host_appearance_query_on_focus: bool,
    split_coalesced_escape: bool,
    host_escape_disambiguation_active: bool,
    awaiting_mouse_tail_after: Option<usize>,
}

impl RawInputByteFramer {
    pub(crate) fn for_host_input() -> Self {
        Self {
            split_coalesced_escape: true,
            ..Self::default()
        }
    }

    pub(crate) fn push(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        if data.is_empty() {
            return self.push_extended();
        }
        let mut chunks = Vec::new();
        for part in data.chunks(8192) {
            let part = self.limits.consume(part);
            if part.is_empty() {
                continue;
            }
            self.drop_abandoned_host_color_reply();
            self.buffer.extend_from_slice(part);
            self.enforce_limits();
            chunks.extend(self.push_extended());
            self.partial_host_color_reply_read_at =
                starts_with_incomplete_default_color_response(&self.buffer).then(Instant::now);
        }
        chunks
    }

    fn push_extended(&mut self) -> Vec<Vec<u8>> {
        if let Some(prefix_len) = self.awaiting_mouse_tail_after.take()
            && !continues_escape_sequence(&self.buffer)
        {
            // The prefix already outlived keyboard timing; it was a key.
            let mut chunks = vec![self.buffer.drain(..prefix_len).collect()];
            chunks.extend(self.drain_available_chunks());
            return chunks;
        }
        self.drain_available_chunks()
    }

    /// Hold a lone trailing ESC for one idle flush so an OSC 10/11
    /// reply split at its ESC introducer stitches back together
    /// instead of leaking (#549). Overlapping queries each add their
    /// own replies to the window.
    pub(crate) fn host_color_query_sent(&mut self) {
        self.host_color_query_sent_with_replies(HOST_COLOR_QUERY_REPLIES);
    }

    fn host_color_query_sent_with_replies(&mut self, replies: u16) {
        self.host_color_replies_awaited = self.host_color_replies_awaited.saturating_add(replies);
        self.held_pending_host_reply_esc = false;
        self.host_reply_activity_at = Some(Instant::now());
    }

    /// Whether a buffered unfinished OSC 10/11 reply has waited past
    /// its idle limit or grown past any real reply, so it can no
    /// longer hold input.
    fn host_color_reply_abandoned(&self) -> bool {
        self.buffer.len() >= MAX_DISCARDED_CONTROL_TAIL_BYTES
            || self
                .partial_host_color_reply_read_at
                .is_none_or(|at| at.elapsed() >= HOST_COLOR_REPLY_IDLE_LIMIT)
    }

    fn drop_abandoned_host_color_reply(&mut self) {
        if self.partial_host_color_reply_read_at.is_none()
            || !starts_with_incomplete_default_color_response(&self.buffer)
            || !self.host_color_reply_abandoned()
        {
            return;
        }
        self.buffer.clear();
        self.partial_host_color_reply_read_at = None;
    }

    /// Same hold window as `host_color_query_sent`, for the XTWINOPS
    /// cell size reply.
    pub(crate) fn host_cell_size_query_sent(&mut self) {
        self.host_cell_size_replies_awaited = HOST_CELL_SIZE_QUERY_REPLIES;
        self.held_pending_host_reply_esc = false;
        self.host_reply_activity_at = Some(Instant::now());
    }

    /// Marks the `CSI ? 996 n` appearance query's outstanding reply.
    pub(crate) fn host_appearance_query_sent(&mut self) {
        self.host_appearance_reply_awaited = true;
        self.held_pending_host_reply_esc = false;
        self.host_reply_activity_at = Some(Instant::now());
    }

    fn awaiting_host_reply(&self) -> bool {
        (self.host_color_replies_awaited > 0
            || self.host_cell_size_replies_awaited > 0
            || self.host_appearance_reply_awaited)
            && self
                .host_reply_activity_at
                .is_some_and(|at| at.elapsed() < HOST_REPLY_QUIET_WINDOW)
    }

    /// Stop expecting replies the host never sent, so their reply
    /// handling no longer holds or discards what is now ordinary input.
    fn expire_unanswered_host_queries(&mut self) {
        if self.awaiting_host_reply() || self.host_reply_activity_at.is_none() {
            return;
        }
        self.host_color_replies_awaited = 0;
        self.host_cell_size_replies_awaited = 0;
        self.host_appearance_reply_awaited = false;
        self.held_pending_host_reply_esc = false;
        self.host_reply_activity_at = None;
    }

    #[cfg(test)]
    fn age_partial_host_color_reply(&mut self, by: Duration) {
        self.partial_host_color_reply_read_at = self
            .partial_host_color_reply_read_at
            .and_then(|at| at.checked_sub(by));
    }

    #[cfg(test)]
    fn age_host_reply_activity(&mut self, by: Duration) {
        self.host_reply_activity_at = self
            .host_reply_activity_at
            .and_then(|at| at.checked_sub(by));
    }

    pub(crate) fn enable_host_color_scheme_change_tracking(&mut self) {
        self.host_color_scheme_change_tracking = true;
    }

    /// Arm the bounded host-reply window when focus gain will emit an
    /// appearance query.
    pub(crate) fn enable_host_appearance_query_on_focus(&mut self) {
        self.host_appearance_query_on_focus = true;
    }

    pub(crate) fn has_pending_input(&self) -> bool {
        !self.buffer.is_empty()
    }

    pub(crate) fn set_host_escape_disambiguation_active(&mut self, active: bool) {
        self.host_escape_disambiguation_active = active;
    }

    /// Pending bytes that could still be a key on their own: a lone
    /// ESC or ESC plus one introducer.
    fn pending_could_be_escape_key(&self) -> bool {
        is_escape_key_prefix(&self.buffer)
    }

    /// How long to wait for more input before `flush_timeout`. Only
    /// bytes that could still be a key get the short keyboard window;
    /// anything already inside a sequence waits long enough for split
    /// reads over slow links.
    pub(crate) fn idle_flush_timeout_ms(&self, host_mouse_capture_active: bool) -> i32 {
        if self.buffer.is_empty() {
            return RAW_INPUT_IDLE_FLUSH_TIMEOUT_MS;
        }
        // A reply Herdr asked for can split right after its introducer.
        if self.awaiting_host_reply() && matches!(self.buffer.as_slice(), b"\x1b[" | b"\x1b]") {
            return INCOMPLETE_SEQUENCE_FLUSH_TIMEOUT_MS;
        }
        if self.pending_could_be_escape_key() {
            if !host_mouse_capture_active {
                return RAW_INPUT_IDLE_FLUSH_TIMEOUT_MS;
            }
            return match self.buffer.as_slice() {
                [ESC] => MOUSE_ACTIVE_ESCAPE_SEQUENCE_FLUSH_TIMEOUT_MS,
                // A mouse report split after ESC[ is still ambiguous
                // with legacy Alt+[.
                b"\x1b[" => MOUSE_ACTIVE_CSI_INTRODUCER_FLUSH_TIMEOUT_MS,
                _ => RAW_INPUT_IDLE_FLUSH_TIMEOUT_MS,
            };
        }
        if self.buffer.first() == Some(&ESC) || starts_with_incomplete_utf8_char(&self.buffer) {
            return INCOMPLETE_SEQUENCE_FLUSH_TIMEOUT_MS;
        }
        RAW_INPUT_IDLE_FLUSH_TIMEOUT_MS
    }

    /// How long to keep holding input after a first idle flush held it.
    pub(crate) fn held_input_flush_timeout_ms(&self) -> i32 {
        if self.awaiting_mouse_tail_after.is_some() {
            DISAMBIGUATED_MOUSE_TAIL_FLUSH_TIMEOUT_MS
        } else {
            RAW_INPUT_IDLE_FLUSH_TIMEOUT_MS
        }
    }

    fn retain_timed_out_mouse_prefix(&mut self, prefix: Vec<u8>) {
        self.timed_out_mouse_prefix = (prefix.len() < MAX_DISCARDED_CONTROL_TAIL_BYTES
            && plausible_sgr_mouse_prefix(&prefix))
        .then_some(prefix);
    }

    fn drain_available_chunks(&mut self) -> Vec<Vec<u8>> {
        let mut chunks = Vec::new();

        loop {
            if let Some(prefix) = &self.timed_out_mouse_prefix {
                match classify_sgr_mouse_continuation(prefix, &self.buffer) {
                    SgrMouseContinuation::Incomplete => break,
                    SgrMouseContinuation::Complete(len) => {
                        self.buffer.drain(..len);
                    }
                    SgrMouseContinuation::Invalid => {}
                }
                self.timed_out_mouse_prefix = None;
            }

            if self.lone_escape_recently_flushed {
                if starts_with_incomplete_orphaned_sgr_mouse_tail(&self.buffer) {
                    break;
                }
                if discard_complete_orphaned_sgr_mouse_tail(&mut self.buffer) {
                    self.lone_escape_recently_flushed = false;
                    continue;
                }
                self.lone_escape_recently_flushed = false;
            }

            if let Some(family) = self.discard_until {
                if family == ControlStringFamily::HostReplyCsi {
                    if discard_host_reply_csi_tail(&mut self.buffer, &mut self.discarded_tail_bytes)
                    {
                        self.discard_until = None;
                        self.discarded_tail_bytes = 0;
                        continue;
                    }
                    break;
                }
                let Some(terminator_len) =
                    control_string_terminator_for_family(&self.buffer, family)
                else {
                    break;
                };
                self.buffer.drain(..terminator_len);
                self.discard_until = None;
                self.discarded_tail_bytes = 0;
                continue;
            }

            // Kitty key reports carry Alt in their modifiers and are
            // never Alt-prefixed, so ESC before a complete report is
            // its own key. WezTerm sends an Escape press as bare ESC
            // and the release as a report, and a quick tap delivers
            // both together (#1266).
            if self.buffer.starts_with(b"\x1b\x1b")
                && (self.split_coalesced_escape || starts_with_kitty_key_report(&self.buffer[1..]))
            {
                chunks.push(vec![ESC]);
                self.buffer.drain(..1);
                continue;
            }

            let Some((event, consumed)) = crate::raw_events::extract_one_event(&self.buffer) else {
                break;
            };
            use crate::host::HostReply as H;
            if matches!(
                event,
                Decoded::Reply(H::Foreground(_) | H::Background(_) | H::Palette(..))
            ) {
                self.host_color_replies_awaited = self.host_color_replies_awaited.saturating_sub(1);
            } else if matches!(event, Decoded::Reply(H::CellSize { .. })) {
                self.host_cell_size_replies_awaited =
                    self.host_cell_size_replies_awaited.saturating_sub(1);
            } else if self.host_appearance_query_on_focus
                && matches!(event, Decoded::Ui(crate::app::UiEvent::Focus(true)))
            {
                self.host_appearance_query_sent();
            } else if matches!(event, Decoded::Reply(H::Dark | H::Light)) {
                self.host_appearance_reply_awaited = false;
                if self.host_color_scheme_change_tracking {
                    self.host_color_query_sent();
                }
            }
            if is_host_reply_event(&event) && self.host_reply_activity_at.is_some() {
                // Long reply bursts (256 palette entries) stay protected.
                self.host_reply_activity_at = Some(Instant::now());
            }
            self.held_pending_host_reply_esc = false;
            chunks.push(self.buffer[..consumed].to_vec());
            self.buffer.drain(..consumed);
        }

        chunks
    }
}

fn is_host_reply_event(event: &Decoded) -> bool {
    matches!(event, Decoded::Reply(_))
}

#[cfg(test)]
mod tests;

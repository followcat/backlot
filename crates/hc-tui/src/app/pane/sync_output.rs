// Derived from herdr src/ghostty/terminal and herdr-core
// crates/herdr-core/src/screen_status.rs (Apache-2.0).
//!
//! Synchronized output (DECSET 2026): a program brackets a frame with
//! `CSI ? 2026 h` … `CSI ? 2026 l` so a terminal never shows the frame
//! half written. hc-tui holds the program's bytes between the two markers
//! and feeds them only at the reset, so the pane's VT keeps the last
//! complete frame — cells and cursor — until the frame closes.
//!
//! The hold is bounded twice: a frame that never resets is released after
//! `SYNC_OUTPUT_TIMEOUT` (Ghostty's value) and a frame larger than
//! `SYNC_OUTPUT_MAX_HELD` is released at once. Either way the pane paints
//! again instead of freezing behind a broken program.
use super::*;

/// How long a frame may stay open before its bytes are fed anyway.
/// Ghostty uses 1 s; a real program's frame is a refresh interval
/// (33 ms at 30 fps), so this is ~30 missed frames — long enough that a
/// slow link never splits a frame, short enough that a program which
/// forgets the reset costs one visible second, not a frozen pane.
pub(super) const SYNC_OUTPUT_TIMEOUT: Duration = Duration::from_secs(1);
/// Most bytes a single frame may hold before it is fed regardless.
pub(super) const SYNC_OUTPUT_MAX_HELD: usize = 256 * 1024;

/// Both markers share this prefix; the byte after it is `h` or `l`. A
/// chunk can end with up to the whole prefix and still be mid-marker.
const PREFIX: &[u8] = b"\x1b[?2026";
/// `ESC [ ? 2026 h` and `ESC [ ? 2026 l`.
const MARKER_LEN: usize = PREFIX.len() + 1;

/// The split state: bytes received but not yet fed, and whether a frame
/// is holding them.
#[derive(Default)]
pub(super) struct SyncOutput {
    /// Everything not fed yet: the open frame, plus a trailing partial
    /// marker while no frame is open.
    buffer: Vec<u8>,
    /// A frame is open: `2026h` was fed and `2026l` has not arrived.
    open: bool,
    /// When the open frame began — the timeout's clock.
    since: Option<Instant>,
}

impl SyncOutput {
    /// Appends one DATA chunk; returns the bytes that may be fed now, in
    /// wire order. Empty while a frame is held.
    pub(super) fn push(&mut self, bytes: &[u8], now: Instant) -> Vec<u8> {
        self.buffer.extend_from_slice(bytes);
        // A frame too big to keep: a real one is a screenful, so this is
        // a broken or hostile program — paint what it wrote.
        if self.open && self.buffer.len() >= SYNC_OUTPUT_MAX_HELD {
            return self.flush();
        }
        let mut feed_to = 0usize;
        let mut scan = 0usize;
        while let Some((at, begin)) = next_marker(&self.buffer, scan) {
            let end = at + MARKER_LEN;
            if begin {
                if !self.open {
                    // Feed the mode set itself: the VT answers DECRQM
                    // from its own state, and `mode(2026)` is true while
                    // the rest of the frame waits here.
                    self.open = true;
                    self.since = Some(now);
                    feed_to = end;
                }
                // A repeated set inside a frame does not restart it.
            } else {
                // The reset ends the frame: everything up to it is fed.
                self.open = false;
                self.since = None;
                feed_to = end;
            }
            scan = end;
        }
        if !self.open {
            // Nothing is holding the tail: feed it, minus a suffix that
            // may still become a marker in the next chunk.
            let keep = partial_marker_len(&self.buffer[feed_to..]);
            feed_to = self.buffer.len() - keep;
        }
        self.take(feed_to)
    }

    /// The instant the hold's bounded wait expires, while one is open.
    pub(super) fn wake(&self) -> Option<Instant> {
        self.since.map(|since| since + SYNC_OUTPUT_TIMEOUT)
    }

    /// Past its bound: the held bytes to feed now, or `None` when the
    /// frame is still inside it.
    pub(super) fn release(&mut self, now: Instant) -> Option<Vec<u8>> {
        let due = self.wake()?;
        (now >= due).then(|| self.flush())
    }

    /// A new wire: held bytes belong to the old program and never paint.
    pub(super) fn clear(&mut self) {
        self.buffer.clear();
        self.open = false;
        self.since = None;
    }

    /// `true` while a frame is held (tests and diagnostics).
    #[cfg(test)]
    pub(super) fn holding(&self) -> bool {
        self.open
    }

    fn flush(&mut self) -> Vec<u8> {
        self.open = false;
        self.since = None;
        std::mem::take(&mut self.buffer)
    }

    fn take(&mut self, to: usize) -> Vec<u8> {
        if to == 0 {
            return Vec::new();
        }
        self.buffer.drain(..to).collect()
    }
}

/// The first `2026h`/`2026l` at or after `from`, as `(offset, begin)`.
fn next_marker(bytes: &[u8], from: usize) -> Option<(usize, bool)> {
    let mut at = from;
    while at + PREFIX.len() <= bytes.len() {
        if &bytes[at..at + PREFIX.len()] == PREFIX {
            match bytes.get(at + PREFIX.len()) {
                Some(b'h') => return Some((at, true)),
                Some(b'l') => return Some((at, false)),
                _ => {}
            }
        }
        at += 1;
    }
    None
}

/// Length of the longest suffix of `bytes` that is a proper prefix of a
/// marker — bytes the next chunk may complete.
fn partial_marker_len(bytes: &[u8]) -> usize {
    let max = (PREFIX.len() - 1).min(bytes.len());
    (1..=max)
        .rev()
        .find(|len| bytes[bytes.len() - len..] == PREFIX[..*len])
        .unwrap_or(0)
}

/// Feeds one DATA chunk: a synchronized frame is held back until it
/// closes, so the VT is only ever a complete frame; an armed observer
/// repaint absorbs what it receives and restores appearance at its own
/// marker. Free of `Pane` so the caller can keep its `Live` borrow.
pub(super) fn feed(
    sync: &mut SyncOutput,
    live: &mut Live,
    vt: &mut Option<Vt>,
    bytes: &[u8],
    now: Instant,
) {
    let ready = sync.push(bytes, now);
    if ready.is_empty() {
        return;
    }
    let Some(vt) = vt.as_mut() else {
        return;
    };
    {
        if let Some(repaint) = &mut live.repaint {
            match repaint.feed(vt, &ready, now) {
                Ok(repaint::Release::Marker) => live.repaint = None,
                Ok(repaint::Release::Fallback) => {
                    live.repaint = None;
                    live.paint_fallback = Some(paint_gate::PAINT_FALLBACK_REASON);
                }
                Ok(repaint::Release::Waiting) => {}
                Err(error) => {
                    live.repaint = None;
                    live.vt_broken = Some(format!("appearance restore: {error}"));
                }
            }
        } else {
            vt.feed(&ready);
        }
    }
}

impl Pane {
    /// Feeds live program bytes into the pane's VT.
    pub(super) fn feed_output(&mut self, bytes: &[u8], now: Instant) {
        if let State::Live(live) = &mut self.state {
            feed(&mut self.sync_output, live, &mut self.vt, bytes, now);
        }
    }

    /// Feeds a frame whose bounded wait expired. `true` when bytes landed
    /// — the pane must repaint.
    pub(super) fn fire_sync_output(&mut self, now: Instant) -> bool {
        let Some(bytes) = self.sync_output.release(now) else {
            return false;
        };
        if bytes.is_empty() {
            return false;
        }
        self.feed_output(&bytes, now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split() -> SyncOutput {
        SyncOutput::default()
    }

    /// One frame written in pieces: only the reset releases it, and the
    /// released bytes are the whole frame in wire order.
    #[test]
    fn a_frame_in_pieces_is_released_at_the_reset() {
        let mut sync = split();
        let now = Instant::now();
        assert_eq!(sync.push(b"\x1b[?2026h\x1b[H\x1b[2J", now), b"\x1b[?2026h");
        assert!(sync.holding());
        assert!(sync.push(b"half the rows", now).is_empty());
        assert!(sync.push(b" the rest", now).is_empty());
        assert_eq!(
            sync.push(b"\x1b[?2026l", now),
            b"\x1b[H\x1b[2Jhalf the rows the rest\x1b[?2026l"
        );
        assert!(!sync.holding());
    }

    /// `2026l` then the next frame's `2026h` in one chunk: the complete
    /// first frame is fed, and the second one starts holding.
    #[test]
    fn two_frames_in_one_chunk_feed_through_the_first_reset() {
        let mut sync = split();
        let now = Instant::now();
        let chunk = b"\x1b[?2026hfirst\x1b[?2026l\x1b[?2026hsecond";
        assert_eq!(
            sync.push(chunk, now),
            b"\x1b[?2026hfirst\x1b[?2026l\x1b[?2026h"
        );
        assert!(sync.holding());
        assert_eq!(sync.push(b"\x1b[?2026l", now), b"second\x1b[?2026l");
    }

    /// A marker split across chunks is bridged, never fed early and never
    /// mistaken for something else.
    #[test]
    fn a_marker_split_across_chunks_is_bridged() {
        let mut sync = split();
        let now = Instant::now();
        assert_eq!(sync.push(b"\x1b[?2026hframe\x1b[?20", now), b"\x1b[?2026h");
        assert!(sync.holding());
        assert_eq!(sync.push(b"26l", now), b"frame\x1b[?2026l");
        assert!(!sync.holding());
    }

    /// A partial prefix that turns out to be another sequence is fed as
    /// soon as it is resolved — not swallowed as a marker.
    #[test]
    fn a_partial_prefix_that_is_not_a_marker_is_fed() {
        let mut sync = split();
        let now = Instant::now();
        assert_eq!(sync.push(b"text\x1b[?20", now), b"text");
        assert_eq!(sync.push(b"04hmore", now), b"\x1b[?2004hmore");
        assert!(!sync.holding());
    }

    /// A reset with no frame open is just bytes; a repeated set does not
    /// restart the hold.
    #[test]
    fn a_reset_without_a_frame_and_a_repeated_set_are_harmless() {
        let mut sync = split();
        let now = Instant::now();
        assert_eq!(sync.push(b"a\x1b[?2026lb", now), b"a\x1b[?2026lb");
        assert_eq!(sync.push(b"\x1b[?2026hone\x1b[?2026h", now), b"\x1b[?2026h");
        let due = sync.wake().expect("the first set armed the clock");
        assert_eq!(
            sync.push(b"two\x1b[?2026l", now),
            b"one\x1b[?2026htwo\x1b[?2026l"
        );
        assert!(!sync.holding());
        let _ = due;
    }

    /// A frame that never resets is released at the timeout, and the
    /// clock is `SYNC_OUTPUT_TIMEOUT` from the set.
    #[test]
    fn the_timeout_releases_a_frame_that_never_resets() {
        let mut sync = split();
        let now = Instant::now();
        sync.push(b"\x1b[?2026hstuck", now);
        let due = sync.wake().expect("a hold arms the clock");
        assert_eq!(due, now + SYNC_OUTPUT_TIMEOUT);
        assert_eq!(due, now + Duration::from_secs(1));
        assert!(sync.release(due - Duration::from_millis(1)).is_none());
        assert_eq!(sync.release(due), Some(b"stuck".to_vec()));
        assert!(!sync.holding());
        assert!(sync.release(due).is_none());
    }

    /// A frame past the byte cap is fed at once, so a hostile program
    /// cannot grow the client's memory with one open frame.
    #[test]
    fn a_frame_past_the_byte_cap_is_fed_immediately() {
        let mut sync = split();
        let now = Instant::now();
        sync.push(b"\x1b[?2026h", now);
        let big = vec![b'x'; SYNC_OUTPUT_MAX_HELD];
        assert_eq!(sync.push(&big, now).len(), SYNC_OUTPUT_MAX_HELD);
        assert!(!sync.holding());
    }
}

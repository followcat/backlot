//! The `?9001h/l` gate's bounded wait. The marker stays the primary
//! release for initial paint and observer repaints, but a core that
//! omits it must not pin queued input, the retained screen or the
//! core-grid size forever: once DATA is flowing, the deadline or the
//! byte cap releases the gate as a fallback.
use super::*;

/// Longest the gate waits once DATA is flowing — an idle shell's
/// single prompt chunk must still release human input.
pub(super) const PAINT_FALLBACK_DELAY: Duration = Duration::from_millis(200);
/// Most DATA bytes the gate holds input through before releasing anyway.
pub(super) const PAINT_FALLBACK_MAX_BYTES: usize = 256 * 1024;
/// Surfaced on the status line when a paint wait released without `?9001`.
pub(super) const PAINT_FALLBACK_REASON: &str = "paint marker omitted";

/// How a paint wait released.
#[derive(Clone, Copy)]
pub(super) enum GateEnd {
    /// `?9001h/l` arrived — the normal release.
    Marker,
    /// The byte cap or the deadline released the gate without a marker.
    Fallback,
}

/// Initial-paint gate progress: the split-marker tail plus fallback
/// state. `await_paint` lives here so every consumer reads one truth.
pub(super) struct PaintGate {
    /// No `?9001` and no fallback yet: input waits, screen retained.
    await_paint: bool,
    /// Up to 7 trailing bytes, bridging a marker split across chunks.
    tail: Vec<u8>,
    /// Armed on the first DATA chunk — never at attach open, so a slow
    /// link cannot time the gate out before history starts arriving.
    deadline: Option<Instant>,
    /// DATA bytes scanned while waiting — the byte-cap half.
    bytes: usize,
}

impl PaintGate {
    pub(super) fn new() -> Self {
        Self {
            await_paint: true,
            tail: Vec::new(),
            deadline: None,
            bytes: 0,
        }
    }

    /// The retained-screen/input gate is still shut.
    pub(super) fn await_paint(&self) -> bool {
        self.await_paint
    }

    /// The armed fallback deadline while the gate is shut.
    pub(super) fn wake(&self) -> Option<Instant> {
        if self.await_paint {
            self.deadline
        } else {
            None
        }
    }

    /// One DATA chunk while the gate is shut: the marker releases
    /// normally; otherwise the clock arms once, bytes accumulate, and
    /// either bound releases the gate via the bounded fallback.
    pub(super) fn feed(&mut self, bytes: &[u8], now: Instant) -> Option<GateEnd> {
        if !self.await_paint {
            return None;
        }
        self.deadline.get_or_insert(now + PAINT_FALLBACK_DELAY);
        self.tail.extend_from_slice(bytes);
        if self
            .tail
            .windows(8)
            .any(|part| part == b"\x1b[?9001h" || part == b"\x1b[?9001l")
        {
            self.await_paint = false;
            self.tail.clear();
            return Some(GateEnd::Marker);
        }
        if self.tail.len() > 7 {
            let keep = self.tail.len() - 7;
            self.tail.drain(..keep);
        }
        self.bytes = self.bytes.saturating_add(bytes.len());
        if self.bytes >= PAINT_FALLBACK_MAX_BYTES || self.deadline.is_some_and(|d| now >= d) {
            self.await_paint = false;
            self.tail.clear();
            return Some(GateEnd::Fallback);
        }
        None
    }

    /// The timer half: releases the shut gate past its armed deadline.
    pub(super) fn fire(&mut self, now: Instant) -> bool {
        if self.await_paint && self.deadline.is_some_and(|d| now >= d) {
            self.await_paint = false;
            self.tail.clear();
            true
        } else {
            false
        }
    }
}

impl Pane {
    /// Releases the live wire's paint waits whose deadlines passed —
    /// the initial gate and an observer repaint. Queued input then
    /// encodes under the VT as it stands: a missing marker must never
    /// pin human input forever.
    pub(super) fn fire_paint_fallback(&mut self, now: Instant) -> bool {
        let released = {
            let State::Live(live) = &mut self.state else {
                return false;
            };
            let mut released = false;
            if live.paint_gate.fire(now) {
                live.paint_fallback = Some(PAINT_FALLBACK_REASON);
                if let Some(vt) = &mut self.vt
                    && vt.size() != Ok(live.core_grid)
                    && let Err(error) = vt.resize(live.core_grid.0, live.core_grid.1)
                {
                    live.vt_broken = Some(format!("resize: {error}"));
                }
                released = true;
            }
            if live
                .repaint
                .as_ref()
                .is_some_and(|repaint| now >= repaint.deadline())
                && let Some(mut repaint) = live.repaint.take()
            {
                if let Some(vt) = &mut self.vt {
                    repaint.finish_without_marker(vt);
                }
                live.paint_fallback = Some(PAINT_FALLBACK_REASON);
                released = true;
            }
            released
        };
        if released {
            self.flush_pending();
        }
        released
    }
}

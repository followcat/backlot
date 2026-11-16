//! Observer resize paint includes DECSTR, which resets live cursor/palette.
//! Restore omitted appearance at the mode barrier, before subsequent PTY data.
//! A core that omits `?9001` releases through the same bounded fallback as
//! the initial paint gate — its held tail then flows in as live bytes.
use hc_vt::{Appearance, Vt};
use std::time::Instant;

use super::paint_gate::{PAINT_FALLBACK_DELAY, PAINT_FALLBACK_MAX_BYTES};

pub(super) struct Repaint {
    saved: Appearance,
    tail: Vec<u8>,
    /// DATA bytes scanned while waiting — the byte-cap half.
    bytes: usize,
    /// Armed at creation so a resize whose repaint never arrives (or
    /// arrives without `?9001`) still releases on time.
    deadline: Instant,
}

/// How a repaint wait resolved after a DATA chunk.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Release {
    /// `?9001h/l` arrived — appearance restored before live bytes.
    Marker,
    /// Still holding up to 7 tail bytes for a split marker.
    Waiting,
    /// The bounded fallback released without a marker; tail flushed.
    Fallback,
}

fn barrier(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(8)
        .position(|p| p == b"\x1b[?9001h" || p == b"\x1b[?9001l")
        .map(|at| at + 8)
}

impl Repaint {
    pub fn new(vt: &mut Vt, now: Instant) -> Result<Self, hc_vt::Error> {
        Ok(Self {
            saved: vt.capture_appearance()?,
            tail: Vec::new(),
            bytes: 0,
            deadline: now + PAINT_FALLBACK_DELAY,
        })
    }

    /// The instant the timer path may release this repaint.
    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    /// A second RESIZE while this repaint still waits: the held tail
    /// flushed already; the bounded wait starts over.
    pub fn reset(&mut self, now: Instant) {
        self.bytes = 0;
        self.deadline = now + PAINT_FALLBACK_DELAY;
    }

    /// Releases without `?9001`: the held tail is live bytes, fed now.
    /// No appearance restore runs — the repaint frame's own bytes
    /// stand as the last word on cursor and palette.
    pub fn finish_without_marker(&mut self, vt: &mut Vt) {
        self.flush_tail(vt);
    }

    pub fn flush_tail(&mut self, vt: &mut Vt) {
        vt.feed(&std::mem::take(&mut self.tail));
    }

    /// Held-tail state after a chunk without the marker: still waiting,
    /// or released by the bounded fallback with the tail fed live.
    fn waiting(&mut self, vt: &mut Vt, now: Instant) -> Release {
        if self.bytes >= PAINT_FALLBACK_MAX_BYTES || now >= self.deadline {
            self.flush_tail(vt);
            Release::Fallback
        } else {
            Release::Waiting
        }
    }

    pub fn feed(
        &mut self,
        vt: &mut Vt,
        bytes: &[u8],
        now: Instant,
    ) -> Result<Release, hc_vt::Error> {
        self.feed_with(vt, bytes, now, |vt, saved| vt.restore_appearance(saved))
    }

    fn feed_with(
        &mut self,
        vt: &mut Vt,
        bytes: &[u8],
        now: Instant,
        restore: impl FnOnce(&mut Vt, &Appearance) -> Result<(), hc_vt::Error>,
    ) -> Result<Release, hc_vt::Error> {
        self.bytes = self.bytes.saturating_add(bytes.len());
        let old = self.tail.len();
        let prefix = bytes.len().min(7);
        let mut bridge = std::mem::take(&mut self.tail);
        bridge.extend_from_slice(&bytes[..prefix]);
        if let Some(end) = barrier(&bridge) {
            vt.feed(&bridge[..end]);
            let restored = restore(vt, &self.saved);
            vt.feed(&bridge[end..]);
            vt.feed(&bytes[prefix..]);
            return restored.map(|()| Release::Marker);
        }
        if bytes.len() <= 7 {
            let keep = bridge.len().saturating_sub(7);
            vt.feed(&bridge[..keep]);
            self.tail.extend_from_slice(&bridge[keep..]);
            return Ok(self.waiting(vt, now));
        }
        vt.feed(&bridge[..old]);
        if let Some(end) = barrier(bytes) {
            vt.feed(&bytes[..end]);
            let restored = restore(vt, &self.saved);
            vt.feed(&bytes[end..]);
            return restored.map(|()| Release::Marker);
        }
        let keep = bytes.len() - 7;
        vt.feed(&bytes[..keep]);
        self.tail.extend_from_slice(&bytes[keep..]);
        Ok(self.waiting(vt, now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_paint_restores_appearance_before_a_live_tail_reset() {
        let now = Instant::now();
        for split in 0..=19 {
            let mut vt = Vt::new(80, 24, 0);
            vt.feed(b"\x1b[5 q\x1b]4;1;#345678\x07");
            let before = vt.frame(0).unwrap();
            let mut paint = Repaint::new(&mut vt, now).unwrap();
            let bytes = b"\x18\x1b[!p\x1b[?9001l";
            let split = split.min(bytes.len());
            let ended = paint.feed(&mut vt, &bytes[..split], now).unwrap();
            if ended == Release::Waiting {
                assert_eq!(
                    paint.feed(&mut vt, &bytes[split..], now).unwrap(),
                    Release::Marker
                );
            }
            let after = vt.frame(0).unwrap();
            assert_eq!(after.cursor.shape, before.cursor.shape);
            assert_eq!(after.colors.palette[1], before.colors.palette[1]);
        }
        let mut vt = Vt::new(80, 24, 0);
        vt.feed(b"\x1b[5 q\x1b]4;1;#345678\x07");
        let mut paint = Repaint::new(&mut vt, now).unwrap();
        assert_eq!(
            paint
                .feed(
                    &mut vt,
                    b"\x18\x1b[!p\x1b[?9001l\x1b[0 q\x1b]104;1\x07",
                    now
                )
                .unwrap(),
            Release::Marker
        );
        assert!(!vt.frame(0).unwrap().cursor.shape_overridden);
        assert_ne!(
            vt.frame(0).unwrap().colors.palette[1],
            hc_vt::Rgb {
                r: 0x34,
                g: 0x56,
                b: 0x78
            }
        );
    }
}

#[cfg(test)]
mod large_tests {
    use super::*;
    #[test]
    fn paint_spanning_a_frame_boundary_restores_before_live_tail() {
        let now = Instant::now();
        let mut vt = Vt::new(80, 24, 0);
        vt.feed(b"\x1b[5 q\x1b]4;1;#123456\x07");
        let mut paint = Repaint::new(&mut vt, now).unwrap();
        vt.resize(40, 20).unwrap();
        let mut bytes = b"\x18\x1b[!p".to_vec();
        bytes.resize(16380, b' ');
        bytes.extend_from_slice(b"\x1b[?9001l\x1b[0 q\x1b]104\x07");
        assert_eq!(
            paint.feed(&mut vt, &bytes[..16384], now).unwrap(),
            Release::Waiting
        );
        assert!(paint.tail.len() <= 7);
        assert_eq!(
            paint.feed(&mut vt, &bytes[16384..], now).unwrap(),
            Release::Marker
        );
        let frame = vt.frame(0).unwrap();
        assert!(!frame.cursor.shape_overridden);
        assert_ne!(
            frame.colors.palette[1],
            hc_vt::Rgb {
                r: 0x12,
                g: 0x34,
                b: 0x56
            }
        );
    }
}

#[cfg(test)]
mod error_tests {
    use super::*;
    #[test]
    fn restoration_failure_still_feeds_live_bytes_on_both_boundary_paths() {
        let now = Instant::now();
        let bytes = b"\x18\x1b[!p\x1b[?9001l\x1b[0 qtail";
        for split in [0, 10] {
            let mut vt = Vt::new(80, 24, 0);
            vt.feed(b"\x1b[5 q");
            let mut paint = Repaint::new(&mut vt, now).unwrap();
            assert_eq!(
                paint.feed(&mut vt, &bytes[..split], now).unwrap(),
                Release::Waiting
            );
            assert!(
                paint
                    .feed_with(&mut vt, &bytes[split..], now, |_, _| Err(
                        hc_vt::Error::Ghostty(-2)
                    ))
                    .is_err()
            );
            let frame = vt.frame(0).unwrap();
            assert!(!frame.cursor.shape_overridden);
            let text = frame
                .rows
                .iter()
                .flat_map(|row| row.cells.iter().map(|cell| cell.text.as_str()))
                .collect::<String>();
            assert!(text.contains("tail"));
        }
    }
}

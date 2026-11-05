//! Observer resize paint includes DECSTR, which resets live cursor/palette.
//! Restore omitted appearance at the mode barrier, before subsequent PTY data.
use hc_vt::{Appearance, Vt};

pub(super) struct Repaint {
    saved: Appearance,
    tail: Vec<u8>,
}

fn barrier(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(8)
        .position(|p| p == b"\x1b[?9001h" || p == b"\x1b[?9001l")
        .map(|at| at + 8)
}

impl Repaint {
    pub fn new(vt: &mut Vt) -> Result<Self, hc_vt::Error> {
        Ok(Self {
            saved: vt.capture_appearance()?,
            tail: Vec::new(),
        })
    }

    pub fn flush_tail(&mut self, vt: &mut Vt) {
        vt.feed(&std::mem::take(&mut self.tail));
    }

    pub fn feed(&mut self, vt: &mut Vt, bytes: &[u8]) -> Result<bool, hc_vt::Error> {
        self.feed_with(vt, bytes, |vt, saved| vt.restore_appearance(saved))
    }

    fn feed_with(
        &mut self,
        vt: &mut Vt,
        bytes: &[u8],
        restore: impl FnOnce(&mut Vt, &Appearance) -> Result<(), hc_vt::Error>,
    ) -> Result<bool, hc_vt::Error> {
        let old = self.tail.len();
        let prefix = bytes.len().min(7);
        let mut bridge = std::mem::take(&mut self.tail);
        bridge.extend_from_slice(&bytes[..prefix]);
        if let Some(end) = barrier(&bridge) {
            vt.feed(&bridge[..end]);
            let restored = restore(vt, &self.saved);
            vt.feed(&bridge[end..]);
            vt.feed(&bytes[prefix..]);
            return restored.map(|()| true);
        }
        if bytes.len() <= 7 {
            let keep = bridge.len().saturating_sub(7);
            vt.feed(&bridge[..keep]);
            self.tail.extend_from_slice(&bridge[keep..]);
            return Ok(false);
        }
        vt.feed(&bridge[..old]);
        if let Some(end) = barrier(bytes) {
            vt.feed(&bytes[..end]);
            let restored = restore(vt, &self.saved);
            vt.feed(&bytes[end..]);
            return restored.map(|()| true);
        }
        let keep = bytes.len() - 7;
        vt.feed(&bytes[..keep]);
        self.tail.extend_from_slice(&bytes[keep..]);
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_paint_restores_appearance_before_a_live_tail_reset() {
        for split in 0..=19 {
            let mut vt = Vt::new(80, 24, 0);
            vt.feed(b"\x1b[5 q\x1b]4;1;#345678\x07");
            let before = vt.frame(0).unwrap();
            let mut paint = Repaint::new(&mut vt).unwrap();
            let bytes = b"\x18\x1b[!p\x1b[?9001l";
            let split = split.min(bytes.len());
            let ended = paint.feed(&mut vt, &bytes[..split]).unwrap();
            if !ended {
                assert!(paint.feed(&mut vt, &bytes[split..]).unwrap());
            }
            let after = vt.frame(0).unwrap();
            assert_eq!(after.cursor.shape, before.cursor.shape);
            assert_eq!(after.colors.palette[1], before.colors.palette[1]);
        }
        let mut vt = Vt::new(80, 24, 0);
        vt.feed(b"\x1b[5 q\x1b]4;1;#345678\x07");
        let mut paint = Repaint::new(&mut vt).unwrap();
        assert!(
            paint
                .feed(&mut vt, b"\x18\x1b[!p\x1b[?9001l\x1b[0 q\x1b]104;1\x07")
                .unwrap()
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
        let mut vt = Vt::new(80, 24, 0);
        vt.feed(b"\x1b[5 q\x1b]4;1;#123456\x07");
        let mut paint = Repaint::new(&mut vt).unwrap();
        vt.resize(40, 20).unwrap();
        let mut bytes = b"\x18\x1b[!p".to_vec();
        bytes.resize(16380, b' ');
        bytes.extend_from_slice(b"\x1b[?9001l\x1b[0 q\x1b]104\x07");
        assert!(!paint.feed(&mut vt, &bytes[..16384]).unwrap());
        assert!(paint.tail.len() <= 7);
        assert!(paint.feed(&mut vt, &bytes[16384..]).unwrap());
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
        let bytes = b"\x18\x1b[!p\x1b[?9001l\x1b[0 qtail";
        for split in [0, 10] {
            let mut vt = Vt::new(80, 24, 0);
            vt.feed(b"\x1b[5 q");
            let mut paint = Repaint::new(&mut vt).unwrap();
            assert!(!paint.feed(&mut vt, &bytes[..split]).unwrap());
            assert!(
                paint
                    .feed_with(&mut vt, &bytes[split..], |_, _| Err(hc_vt::Error::Ghostty(
                        -2
                    )))
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

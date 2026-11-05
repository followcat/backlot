// Derived in part from herdr src/pane/terminal.rs (Apache-2.0).
//! Preserve known live appearance across an observer's synthetic resize paint.
//! Uses the pinned Ghostty DynamicPalette::changeDefault semantics (MIT).
use crate::{CursorShape, Error, Rgb, Vt, ffi};

pub struct Appearance {
    shape: CursorShape,
    overridden: bool,
    cursor_revision: u64,
    palette: Vec<(usize, Rgb)>,
}

impl Vt {
    /// Snapshot only explicit appearance; this does not acknowledge output or
    /// write anything to the PTY. Palette overrides equal to defaults count.
    pub fn capture_appearance(&mut self) -> Result<Appearance, Error> {
        let frame = self.frame(0)?;
        let palette = self.explicit_palette()?;
        Ok(Appearance {
            shape: frame.cursor.shape,
            overridden: frame.cursor.shape_overridden,
            cursor_revision: self.cursor_tracker.revision(),
            palette,
        })
    }

    /// Apply only omitted fields. An explicit field supplied by the paint wins.
    /// Dynamic OSC 10/11/12 colors already survive DECSTR in the pinned VT.
    pub fn restore_appearance(&mut self, saved: &Appearance) -> Result<(), Error> {
        let explicit = self.explicit_palette()?;
        let mut sequence = String::new();
        use std::fmt::Write;
        for &(index, color) in &saved.palette {
            if !explicit.iter().any(|(supplied, _)| *supplied == index) {
                let _ = write!(
                    sequence,
                    "\x1b]4;{index};rgb:{:02x}/{:02x}/{:02x}\x1b\\",
                    color.r, color.g, color.b
                );
            }
        }
        if saved.overridden && self.cursor_tracker.revision() == saved.cursor_revision {
            let blink = self.frame(0)?.cursor.blinking;
            let style = match saved.shape {
                CursorShape::Bar => {
                    if blink {
                        5
                    } else {
                        6
                    }
                }
                CursorShape::Underline => {
                    if blink {
                        3
                    } else {
                        4
                    }
                }
                _ => {
                    if blink {
                        1
                    } else {
                        2
                    }
                }
            };
            let _ = write!(sequence, "\x1b[{style} q");
        }
        self.feed(sequence.as_bytes());
        Ok(())
    }

    fn native_palette(&self, kind: i32) -> Result<[Rgb; 256], Error> {
        let mut raw = [ffi::Rgb { r: 0, g: 0, b: 0 }; 256];
        let result = unsafe { ffi::ghostty_terminal_get(self.raw, kind, raw.as_mut_ptr().cast()) };
        if result != ffi::SUCCESS {
            return Err(Error::Ghostty(result));
        }
        Ok(raw.map(Rgb::from))
    }

    /// The render API omits the override mask. A temporary default change
    /// exposes it: masked entries keep their current value; unmasked entries
    /// follow the probe. Restore defaults before returning, including errors.
    /// `&mut Vt` prevents any draw/input from observing the temporary defaults.
    fn explicit_palette(&mut self) -> Result<Vec<(usize, Rgb)>, Error> {
        let current = self.native_palette(ffi::DATA_COLOR_PALETTE)?;
        let original = self.native_palette(ffi::DATA_COLOR_PALETTE_DEFAULT)?;
        let mut builtin = [ffi::Rgb { r: 0, g: 0, b: 0 }; 256];
        unsafe {
            ffi::ghostty_color_palette_default(builtin.as_mut_ptr());
        }
        let mut probe = current.map(|color| Rgb {
            r: color.r ^ 0x80,
            ..color
        });
        // Avoid the built-in default fast path: it frees the owned palette.
        // Keeping that allocation makes restoration to custom defaults reuse
        // it; restoration to built-in defaults never needs an allocation.
        if probe[0] == Rgb::from(builtin[0]) {
            probe[0].g ^= 0x80;
        }
        self.set_default_colors(None, None, Some(&probe))?;
        let sampled = self.native_palette(ffi::DATA_COLOR_PALETTE);
        let restored = self.set_default_colors(None, None, Some(&original));
        restored?;
        let sampled = sampled?;
        Ok(current
            .into_iter()
            .enumerate()
            .filter(|(index, color)| sampled[*index] == *color)
            .collect())
    }
}

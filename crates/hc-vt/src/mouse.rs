// Derived from herdr src/pane/input.rs and src/pane/terminal.rs (Apache-2.0).
//! Use the live terminal's tracking and encoding, with Source geometry rules.
use std::ptr;

use crate::{Error, Mods, Vt, ffi};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    Motion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    WheelUp,
    WheelDown,
    WheelLeft,
    WheelRight,
}

/// Zero-based position relative to the terminal content, before encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MousePosition {
    Cell { column: u16, row: u16 },
    Pixels { x: u32, y: u32 },
}

#[derive(Debug, Clone, Copy)]
pub struct MouseInput {
    pub action: MouseAction,
    pub button: Option<MouseButton>,
    pub mods: Mods,
    pub position: MousePosition,
}

struct Encoder(ffi::MouseEncoder);
struct Event(ffi::MouseEvent);
impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe { ffi::ghostty_mouse_encoder_free(self.0) }
    }
}
impl Drop for Event {
    fn drop(&mut self) {
        unsafe { ffi::ghostty_mouse_event_free(self.0) }
    }
}

impl Vt {
    /// Encode once per event, as herdr does. In particular, do not deduplicate
    /// repeated motion cells or repeat tracked wheel reports by scroll lines.
    pub fn mouse(&self, input: MouseInput) -> Result<Vec<u8>, Error> {
        if input.action == MouseAction::Motion && input.button.is_none() && !self.mode(1003)? {
            return Ok(Vec::new());
        }
        let mut encoder = Encoder(ptr::null_mut());
        let mut event = Event(ptr::null_mut());
        // SAFETY: RAII owns both C handles; all values/buffers outlive each call.
        unsafe {
            check(ffi::ghostty_mouse_encoder_new(ptr::null(), &mut encoder.0))?;
            check(ffi::ghostty_mouse_event_new(ptr::null(), &mut event.0))?;
            ffi::ghostty_mouse_encoder_setopt_from_terminal(encoder.0, self.raw);
        }
        let (cols, rows) = self.size()?;
        let pixels = self.mode(1016)? || matches!(input.position, MousePosition::Pixels { .. });
        let (cw, ch) = if pixels { self.cell_px() } else { (1, 1) };
        let size = ffi::MouseSize {
            size: std::mem::size_of::<ffi::MouseSize>(),
            screen_width: u32::from(cols) * cw,
            screen_height: u32::from(rows) * ch,
            cell_width: cw,
            cell_height: ch,
            padding_top: 0,
            padding_bottom: 0,
            padding_right: 0,
            padding_left: 0,
        };
        let (x, y) = match input.position {
            MousePosition::Cell { column, row } => {
                if self.mode(1016)? && (column >= cols || row >= rows) {
                    return Ok(Vec::new());
                }
                (
                    u32::from(column) * cw + cw / 2,
                    u32::from(row) * ch + ch / 2,
                )
            }
            MousePosition::Pixels { x, y } => (x, y),
        };
        let action = match input.action {
            MouseAction::Press => 0,
            MouseAction::Release => 1,
            MouseAction::Motion => 2,
        };
        unsafe {
            ffi::ghostty_mouse_encoder_setopt(encoder.0, 2, (&raw const size).cast());
            ffi::ghostty_mouse_event_set_action(event.0, action);
            match input.button {
                Some(button) => ffi::ghostty_mouse_event_set_button(
                    event.0,
                    match button {
                        MouseButton::Left => 1,
                        MouseButton::Right => 2,
                        MouseButton::Middle => 3,
                        MouseButton::WheelUp => 4,
                        MouseButton::WheelDown => 5,
                        MouseButton::WheelLeft => 6,
                        MouseButton::WheelRight => 7,
                    },
                ),
                None => ffi::ghostty_mouse_event_clear_button(event.0),
            }
            ffi::ghostty_mouse_event_set_mods(event.0, input.mods.ghostty());
            ffi::ghostty_mouse_event_set_position(
                event.0,
                ffi::MousePosition {
                    x: x as f32,
                    y: y as f32,
                },
            );
            // Maximum native report is a few decimal u32 fields, well below 128.
            let mut bytes = vec![0u8; 128];
            let mut len = 0;
            check(ffi::ghostty_mouse_encoder_encode(
                encoder.0,
                event.0,
                bytes.as_mut_ptr(),
                bytes.len(),
                &mut len,
            ))?;
            bytes.truncate(len);
            Ok(bytes)
        }
    }
}

fn check(rc: i32) -> Result<(), Error> {
    (rc == ffi::SUCCESS).then_some(()).ok_or(Error::Ghostty(rc))
}

// Derived from Ghostty include/ghostty/vt/mouse/{event,encoder}.h (Apache-2.0).
//! Frozen ABI at the pinned Ghostty submodule revision.
use std::ffi::c_void;

pub type MouseEncoder = *mut c_void;
pub type MouseEvent = *mut c_void;

#[repr(C)]
pub struct MousePosition {
    pub x: f32,
    pub y: f32,
}

#[repr(C)]
pub struct MouseSize {
    pub size: usize,
    pub screen_width: u32,
    pub screen_height: u32,
    pub cell_width: u32,
    pub cell_height: u32,
    pub padding_top: u32,
    pub padding_bottom: u32,
    pub padding_right: u32,
    pub padding_left: u32,
}

unsafe extern "C" {
    pub fn ghostty_mouse_encoder_new(allocator: *const c_void, out: *mut MouseEncoder) -> i32;
    pub fn ghostty_mouse_encoder_free(encoder: MouseEncoder);
    pub fn ghostty_mouse_encoder_setopt(encoder: MouseEncoder, option: i32, value: *const c_void);
    pub fn ghostty_mouse_encoder_setopt_from_terminal(
        encoder: MouseEncoder,
        terminal: super::Terminal,
    );
    pub fn ghostty_mouse_encoder_encode(
        encoder: MouseEncoder,
        event: MouseEvent,
        out: *mut u8,
        capacity: usize,
        len: *mut usize,
    ) -> i32;
    pub fn ghostty_mouse_event_new(allocator: *const c_void, out: *mut MouseEvent) -> i32;
    pub fn ghostty_mouse_event_free(event: MouseEvent);
    pub fn ghostty_mouse_event_set_action(event: MouseEvent, action: i32);
    pub fn ghostty_mouse_event_set_button(event: MouseEvent, button: i32);
    pub fn ghostty_mouse_event_clear_button(event: MouseEvent);
    pub fn ghostty_mouse_event_set_mods(event: MouseEvent, mods: u16);
    pub fn ghostty_mouse_event_set_position(event: MouseEvent, position: MousePosition);
}

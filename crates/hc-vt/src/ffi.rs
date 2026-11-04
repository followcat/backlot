// Derived from herdr-core crates/ghostty-vt/src/lib.rs (Apache-2.0).
//! ABI declarations for libghostty-vt at the pinned submodule revision
//! (vendor/ghostty @ 1e5cdb865807abc990181efb8ebfa9122955cce2). Field order,
//! enum ordinals and sized-struct rules come from
//! `vendor/ghostty/include/ghostty/vt/*.h`; this file is the only unsafe
//! boundary in the crate.

use std::ffi::c_void;

pub type Terminal = *mut c_void;
pub type RenderState = *mut c_void;
pub type RowIterator = *mut c_void;
pub type RowCells = *mut c_void;
pub type KeyEncoder = *mut c_void;
pub type KeyEvent = *mut c_void;

pub const SUCCESS: i32 = 0;
pub const INVALID_VALUE: i32 = -2;
pub const OUT_OF_SPACE: i32 = -3;

// Terminal options (GhosttyTerminalOption).
pub const OPT_USERDATA: i32 = 0;
pub const OPT_WRITE_PTY: i32 = 1;
pub const OPT_SIZE: i32 = 6;
pub const OPT_SCROLLBACK_MAX_BYTES: i32 = 27;
pub const OPT_MODE_DEFAULT: i32 = 33;

// Terminal data (GhosttyTerminalData).
pub const DATA_COLS: i32 = 1;
pub const DATA_ROWS: i32 = 2;
pub const DATA_ACTIVE_SCREEN: i32 = 6;
pub const DATA_MODE: i32 = 37;

// Render-state data (GhosttyRenderStateData).
pub const RS_DIRTY: i32 = 3;
pub const RS_ROW_ITERATOR: i32 = 4;
pub const RS_CURSOR: i32 = 18;
pub const RS_COLORS: i32 = 19;
pub const RS_OVERSCAN: i32 = 20;

// Render-state options (GhosttyRenderStateOption).
pub const RS_OPT_OVERSCAN: i32 = 1;

// Render-state dirty (GhosttyRenderStateDirty).
pub const DIRTY_PARTIAL: i32 = 1;
pub const DIRTY_FULL: i32 = 2;

// Cursor visual styles (GhosttyRenderStateCursorVisualStyle).
pub const CURSOR_BAR: i32 = 0;
pub const CURSOR_UNDERLINE: i32 = 2;
pub const CURSOR_BLOCK_HOLLOW: i32 = 3;

// Row data (GhosttyRenderStateRowData).
pub const ROW_DIRTY: i32 = 1;
pub const ROW_CELLS: i32 = 3;
pub const ROW_VIEWPORT_Y: i32 = 6;

// Row-cell data (GhosttyRenderStateRowCellsData).
pub const CELL_RAW: i32 = 1;
pub const CELL_STYLE: i32 = 2;
pub const CELL_GRAPHEMES_LEN: i32 = 3;
pub const CELL_BG_COLOR: i32 = 5;
pub const CELL_FG_COLOR: i32 = 6;
pub const CELL_GRAPHEMES_UTF8: i32 = 9;

// Packed cell fields (GhosttyCellData).
pub const CELL_DATA_WIDE: i32 = 3;

// GhosttyCellWide.
pub const WIDE_NARROW: i32 = 0;
pub const WIDE_WIDE: i32 = 1;
pub const WIDE_SPACER_TAIL: i32 = 2;
pub const WIDE_SPACER_HEAD: i32 = 3;

// GhosttyStyleColorTag.
pub const COLOR_PALETTE: i32 = 1;
pub const COLOR_RGB: i32 = 2;

// Key event (GhosttyKeyAction / GhosttyMods).
pub const KEY_RELEASE: i32 = 0;
pub const KEY_PRESS: i32 = 1;
pub const KEY_REPEAT: i32 = 2;
pub const MOD_SHIFT: u16 = 1 << 0;
pub const MOD_CTRL: u16 = 1 << 1;
pub const MOD_ALT: u16 = 1 << 2;
pub const MOD_SUPER: u16 = 1 << 3;

// Key encoder options (GhosttyKeyEncoderOption).
pub const ENC_MACOS_OPTION_AS_ALT: i32 = 6;

// GhosttyOptionAsAlt.
pub const OPTION_AS_ALT_TRUE: i32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[repr(C)]
pub struct Buffer {
    pub ptr: *mut u8,
    pub cap: usize,
    pub len: usize,
}

#[repr(C)]
pub struct SizeReport {
    pub rows: u16,
    pub columns: u16,
    pub cell_width: u32,
    pub cell_height: u32,
}

#[repr(C)]
pub struct ModeConfig {
    pub mode: u16,
    pub value: bool,
}

#[repr(C)]
pub struct RenderStateCursor {
    pub size: usize,
    pub viewport_has_value: bool,
    pub viewport_x: u16,
    pub viewport_y: u16,
    pub wide_tail: bool,
    pub visible: bool,
    pub blinking: bool,
    pub password_input: bool,
    pub visual_style: i32,
}

#[repr(C)]
pub struct RenderStateColors {
    pub size: usize,
    pub background: Rgb,
    pub foreground: Rgb,
    pub cursor: Rgb,
    pub cursor_has_value: bool,
    pub palette: [Rgb; 256],
}

#[repr(C)]
pub struct Overscan {
    pub above: u16,
    pub below: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union StyleColorValue {
    pub palette: u8,
    pub rgb: Rgb,
    pub _padding: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct StyleColor {
    pub tag: i32,
    pub value: StyleColorValue,
}

#[repr(C)]
pub struct Style {
    pub size: usize,
    pub fg_color: StyleColor,
    pub bg_color: StyleColor,
    pub underline_color: StyleColor,
    pub bold: bool,
    pub italic: bool,
    pub faint: bool,
    pub blink: bool,
    pub inverse: bool,
    pub invisible: bool,
    pub strikethrough: bool,
    pub overline: bool,
    pub underline: i32,
}

pub type WritePtyFn = unsafe extern "C" fn(Terminal, *mut c_void, *const u8, usize);
pub type SizeFn = unsafe extern "C" fn(Terminal, *mut c_void, *mut SizeReport) -> bool;

unsafe extern "C" {
    pub fn ghostty_terminal_new(
        alloc: *const c_void,
        out: *mut Terminal,
        cols: u16,
        rows: u16,
    ) -> i32;
    pub fn ghostty_terminal_free(t: Terminal);
    pub fn ghostty_terminal_set(t: Terminal, opt: i32, value: *const c_void) -> i32;
    pub fn ghostty_terminal_get(t: Terminal, data: i32, out: *mut c_void) -> i32;
    pub fn ghostty_terminal_vt_write(t: Terminal, data: *const u8, len: usize);
    pub fn ghostty_terminal_resize(
        t: Terminal,
        cols: u16,
        rows: u16,
        cell_px_w: u32,
        cell_px_h: u32,
    ) -> i32;

    pub fn ghostty_render_state_new(alloc: *const c_void, out: *mut RenderState) -> i32;
    pub fn ghostty_render_state_free(s: RenderState);
    pub fn ghostty_render_state_update(s: RenderState, t: Terminal) -> i32;
    pub fn ghostty_render_state_get(s: RenderState, data: i32, out: *mut c_void) -> i32;
    pub fn ghostty_render_state_set(s: RenderState, opt: i32, value: *const c_void) -> i32;
    pub fn ghostty_render_state_clean(s: RenderState) -> i32;

    pub fn ghostty_render_state_row_iterator_new(
        alloc: *const c_void,
        out: *mut RowIterator,
    ) -> i32;
    pub fn ghostty_render_state_row_iterator_free(it: RowIterator);
    pub fn ghostty_render_state_row_iterator_next(it: RowIterator) -> bool;
    pub fn ghostty_render_state_row_get(it: RowIterator, data: i32, out: *mut c_void) -> i32;

    pub fn ghostty_render_state_row_cells_new(alloc: *const c_void, out: *mut RowCells) -> i32;
    pub fn ghostty_render_state_row_cells_free(c: RowCells);
    pub fn ghostty_render_state_row_cells_next(c: RowCells) -> bool;
    pub fn ghostty_render_state_row_cells_get(c: RowCells, data: i32, out: *mut c_void) -> i32;

    pub fn ghostty_cell_get(cell: u64, data: i32, out: *mut c_void) -> i32;

    pub fn ghostty_key_encoder_new(alloc: *const c_void, out: *mut KeyEncoder) -> i32;
    pub fn ghostty_key_encoder_free(e: KeyEncoder);
    pub fn ghostty_key_encoder_setopt(e: KeyEncoder, opt: i32, value: *const c_void);
    pub fn ghostty_key_encoder_setopt_from_terminal(e: KeyEncoder, t: Terminal);
    pub fn ghostty_key_encoder_encode(
        e: KeyEncoder,
        ev: KeyEvent,
        buf: *mut u8,
        cap: usize,
        written: *mut usize,
    ) -> i32;

    pub fn ghostty_key_event_new(alloc: *const c_void, out: *mut KeyEvent) -> i32;
    pub fn ghostty_key_event_free(ev: KeyEvent);
    pub fn ghostty_key_event_set_action(ev: KeyEvent, action: i32);
    pub fn ghostty_key_event_set_key(ev: KeyEvent, key: i32);
    pub fn ghostty_key_event_set_mods(ev: KeyEvent, mods: u16);
    pub fn ghostty_key_event_set_utf8(ev: KeyEvent, utf8: *const u8, len: usize);
    pub fn ghostty_key_event_set_unshifted_codepoint(ev: KeyEvent, cp: u32);

    pub fn ghostty_paste_encode(
        data: *mut u8,
        data_len: usize,
        bracketed: bool,
        buf: *mut u8,
        buf_len: usize,
        out_written: *mut usize,
    ) -> i32;
}

// The sized-struct layouts this file declares must match the pinned C
// headers. These are the x86_64 values measured against those headers;
// other targets may differ, which is why this is a test, not an assert.
#[cfg(all(test, target_pointer_width = "64"))]
mod tests {
    use super::*;
    use std::mem::offset_of;

    #[test]
    fn sized_struct_layouts_match_the_c_probe() {
        assert_eq!(size_of::<Style>(), 72);
        assert_eq!(offset_of!(Style, bold), 56);
        assert_eq!(offset_of!(Style, underline), 64);
        assert_eq!(size_of::<StyleColor>(), 16);
        assert_eq!(offset_of!(StyleColor, value), 8);
        assert_eq!(size_of::<RenderStateCursor>(), 24);
        assert_eq!(offset_of!(RenderStateCursor, visual_style), 20);
        assert_eq!(size_of::<RenderStateColors>(), 792);
        assert_eq!(offset_of!(RenderStateColors, palette), 18);
    }
}

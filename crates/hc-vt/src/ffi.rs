// Derived from herdr-core crates/ghostty-vt/src/lib.rs (Apache-2.0).
//! ABI declarations for libghostty-vt at the pinned submodule revision
//! (vendor/ghostty @ 1e5cdb865807abc990181efb8ebfa9122955cce2). Field order,
//! enum ordinals and sized-struct rules come from
//! `vendor/ghostty/include/ghostty/vt/*.h`. Safe ownership wrappers in
//! the other modules are responsible for valid handles and buffers.

mod mouse;
pub use mouse::*;

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
pub const OPT_BELL: i32 = 2;
pub const OPT_SIZE: i32 = 6;
pub const OPT_CLIPBOARD_WRITE: i32 = 26;
// GHOSTTY_TERMINAL_OPT_COLOR_* — vendor/ghostty/include/ghostty/vt/terminal.h.
pub const OPT_COLOR_FOREGROUND: i32 = 11;
pub const OPT_COLOR_BACKGROUND: i32 = 12;
pub const OPT_COLOR_PALETTE: i32 = 14;
pub const OPT_SCROLLBACK_MAX_BYTES: i32 = 27;
pub const OPT_MODE_DEFAULT: i32 = 33;

// Terminal data (GhosttyTerminalData).
pub const DATA_COLS: i32 = 1;
pub const DATA_ROWS: i32 = 2;
pub const DATA_ACTIVE_SCREEN: i32 = 6;
pub const DATA_COLOR_PALETTE: i32 = 21;
pub const DATA_COLOR_PALETTE_DEFAULT: i32 = 25;
pub const DATA_SCROLLBAR: i32 = 9;
pub const DATA_KITTY_KEYBOARD_FLAGS: i32 = 8;
pub const DATA_MOUSE_TRACKING: i32 = 11;
pub const DATA_TITLE: i32 = 12;

#[repr(C)]
pub struct StringView {
    pub ptr: *const u8,
    pub len: usize,
}
pub const DATA_MODE: i32 = 37;

/// `GHOSTTY_SCROLL_VIEWPORT_*` tags.
pub const SCROLL_DELTA: i32 = 2;
pub const SCROLL_ROW: i32 = 3;

/// `GhosttyTerminalScrollbar` — the viewport's position in scrollback.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Scrollbar {
    /// Total scrollable rows (scrollback + viewport).
    pub total: u64,
    /// Viewport's row offset into `total`.
    pub offset: u64,
    /// Visible rows.
    pub len: u64,
}

/// `GhosttyTerminalScrollViewport` tagged union.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ScrollViewport {
    pub tag: i32,
    pub value: ScrollViewportValue,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union ScrollViewportValue {
    pub delta: isize,
    pub row: usize,
    pub delta_prompt: isize,
    pub _padding: [u64; 2],
}

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
/// `GhosttyTerminalBellFn` — BEL (0x07) notifications, no payload.
pub type BellFn = unsafe extern "C" fn(Terminal, *mut c_void);

// GhosttyClipboardLocation and GhosttyClipboardWriteResult —
// vendor/ghostty/include/ghostty/vt/terminal.h.
pub const CLIPBOARD_LOCATION_STANDARD: i32 = 0;
pub const CLIPBOARD_WRITE_SUCCESS: i32 = 0;
pub const CLIPBOARD_WRITE_UNSUPPORTED: i32 = 2;
pub const CLIPBOARD_WRITE_INVALID_DATA: i32 = 4;

/// `GhosttyClipboardContent`. Borrowed for the callback only.
#[repr(C)]
pub struct ClipboardContent {
    pub mime: StringView,
    pub data: StringView,
}

/// `GhosttyClipboardWriteReply`.
#[repr(C)]
pub struct ClipboardWriteReply {
    pub size: usize,
    pub result: i32,
    pub remember: bool,
}

pub type ClipboardWriteReplyFn =
    unsafe extern "C" fn(*const ClipboardWrite, *const ClipboardWriteReply);

/// `GhosttyClipboardWrite`. A sized struct, borrowed for the callback only.
#[repr(C)]
pub struct ClipboardWrite {
    pub size: usize,
    pub location: i32,
    pub contents: *const ClipboardContent,
    pub contents_len: usize,
    pub name: StringView,
    pub granted: bool,
    pub can_remember: bool,
    pub ctx: *const c_void,
    pub reply: Option<ClipboardWriteReplyFn>,
}

/// `GhosttyTerminalClipboardWriteFn`.
pub type ClipboardWriteFn = unsafe extern "C" fn(Terminal, *mut c_void, *const ClipboardWrite);

pub type Formatter = *mut c_void;

// GhosttyPointTag — vendor/ghostty/include/ghostty/vt/point.h.
pub const POINT_TAG_SCREEN: i32 = 2;

/// `GhosttyPointCoordinate`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PointCoordinate {
    /// Column (0-indexed).
    pub x: u16,
    /// Row (0-indexed). May exceed the viewport for screen/history tags.
    pub y: u32,
}

/// `GhosttyPointValue`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union PointValue {
    pub coordinate: PointCoordinate,
    pub _padding: [u64; 2],
}

impl Default for PointValue {
    fn default() -> Self {
        Self { _padding: [0; 2] }
    }
}

/// `GhosttyPoint` — a point in the terminal grid under a coordinate tag.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Point {
    pub tag: i32,
    pub value: PointValue,
}

/// `GhosttyGridRef` — a snapshot cell reference, valid until the next
/// mutating terminal call. `size` must be set to `size_of::<GridRef>()`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GridRef {
    pub size: usize,
    pub node: *mut c_void,
    pub x: u16,
    pub y: u16,
}

impl Default for GridRef {
    fn default() -> Self {
        Self {
            size: 0,
            node: std::ptr::null_mut(),
            x: 0,
            y: 0,
        }
    }
}

/// `GhosttySelection` — an inclusive endpoint pair; `size` must be set.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Selection {
    pub size: usize,
    pub start: GridRef,
    pub end: GridRef,
    pub rectangle: bool,
}

// GhosttyFormatterFormat — vendor/ghostty/include/ghostty/vt/types.h.
pub const FORMATTER_FORMAT_PLAIN: i32 = 0;

/// `GhosttyFormatterScreenExtra`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FormatterScreenExtra {
    pub size: usize,
    pub cursor: bool,
    pub style: bool,
    pub hyperlink: bool,
    pub protection: bool,
    pub kitty_keyboard: bool,
    pub charsets: bool,
}

/// `GhosttyFormatterTerminalExtra`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FormatterTerminalExtra {
    pub size: usize,
    pub palette: bool,
    pub modes: bool,
    pub scrolling_region: bool,
    pub tabstops: bool,
    pub pwd: bool,
    pub keyboard: bool,
    pub screen: FormatterScreenExtra,
}

/// `GhosttyFormatterTerminalOptions`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FormatterTerminalOptions {
    pub size: usize,
    pub emit: i32,
    pub unwrap: bool,
    pub trim: bool,
    pub extra: FormatterTerminalExtra,
    pub selection: *const Selection,
}

unsafe extern "C" {
    pub fn ghostty_color_palette_default(out: *mut Rgb);
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
    pub fn ghostty_terminal_scroll_viewport(t: Terminal, behavior: ScrollViewport);
    pub fn ghostty_terminal_grid_ref(t: Terminal, point: Point, out: *mut GridRef) -> i32;
    pub fn ghostty_formatter_terminal_new(
        alloc: *const c_void,
        out: *mut Formatter,
        t: Terminal,
        options: FormatterTerminalOptions,
    ) -> i32;
    pub fn ghostty_formatter_format_alloc(
        f: Formatter,
        alloc: *const c_void,
        out_ptr: *mut *mut u8,
        out_len: *mut usize,
    ) -> i32;
    pub fn ghostty_formatter_free(f: Formatter);
    pub fn ghostty_free(alloc: *const c_void, ptr: *mut u8, len: usize);
    pub fn ghostty_unicode_codepoint_width(cp: u32) -> u8;
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
    pub fn ghostty_key_event_set_consumed_mods(ev: KeyEvent, mods: u16);
    pub fn ghostty_key_event_set_composing(ev: KeyEvent, composing: bool);

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
        assert_eq!(size_of::<Point>(), 24);
        assert_eq!(offset_of!(Point, value), 8);
        assert_eq!(size_of::<GridRef>(), 24);
        assert_eq!(offset_of!(GridRef, x), 16);
        assert_eq!(size_of::<Selection>(), 64);
        assert_eq!(offset_of!(Selection, rectangle), 56);
        assert_eq!(size_of::<FormatterScreenExtra>(), 16);
        assert_eq!(offset_of!(FormatterScreenExtra, charsets), 13);
        assert_eq!(size_of::<FormatterTerminalExtra>(), 32);
        assert_eq!(offset_of!(FormatterTerminalExtra, screen), 16);
        assert_eq!(size_of::<FormatterTerminalOptions>(), 56);
        assert_eq!(offset_of!(FormatterTerminalOptions, selection), 48);
    }
}

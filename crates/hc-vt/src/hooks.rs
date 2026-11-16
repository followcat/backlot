// Derived from herdr-core crates/ghostty-vt/src/lib.rs (Apache-2.0).
//! The C callback hooks and `Vt` teardown. Everything the C side can
//! call back into lives here; the rest of the crate sees only owned
//! data through the accessors in `vt.rs`.

use std::ffi::c_void;

use crate::ffi;
use crate::vt::Vt;

/// Bytes the terminal writes to the pty side (query answers, mode
/// reports) are capped: a hostile stream could otherwise grow this
/// without bound while the consumer is not looking. On overflow the
/// latch is set and whole replies are dropped — never partially kept,
/// so a caller can never see a spliced CSI/OSC/DCS fragment.
const REPLIES_MAX_BYTES: usize = 256 * 1024;

/// State the C callbacks receive through the userdata pointer. Owned by
/// `Vt`'s `Box`, so it outlives every callback and is never aliased
/// mutably while C could call in (callbacks only fire during `&mut self`
/// calls on `Vt`).
pub(crate) struct Hooks {
    /// Bytes the terminal wants written back to the pty (query replies,
    /// mode reports). Separate from input the user produces.
    pub(crate) replies: Vec<u8>,
    /// Set when a whole reply was dropped for capacity; cleared when the
    /// caller drains via `take_replies`.
    pub(crate) replies_overflowed: bool,
    /// Reported cell geometry in pixels.
    pub(crate) cell: (u32, u32),
    /// BEL characters seen since the last `take_bell_count`.
    pub(crate) bells: u64,
    /// Text programs copied since the last `take_clipboard_writes`.
    pub(crate) clipboard_writes: Vec<Vec<u8>>,
}

pub(crate) extern "C" fn on_bell(_: ffi::Terminal, ud: *mut c_void) {
    // SAFETY: `ud` is the `Hooks` pointer installed with OPT_USERDATA,
    // valid for the terminal's life. Saturating keeps a hostile BEL flood
    // countable rather than wrapping.
    unsafe {
        let hooks = &mut *(ud as *mut Hooks);
        hooks.bells = hooks.bells.saturating_add(1);
    }
}

/// The largest clipboard write kept, the same limit the reference uses.
const CLIPBOARD_MAX_BYTES: usize = 192 * 1024;

// `on_clipboard_write` and `capture_clipboard_write` are derived from
// herdr crates/ghostty-vt/src/lib.rs (Apache-2.0).
pub(crate) extern "C" fn on_clipboard_write(
    _: ffi::Terminal,
    ud: *mut c_void,
    write: *const ffi::ClipboardWrite,
) {
    // SAFETY: `ud` is the `Hooks` pointer installed with OPT_USERDATA. The
    // library keeps the request alive for this callback, and the size check
    // covers every field read here and in the capture below.
    unsafe {
        if write.is_null() || (*write).size < std::mem::size_of::<ffi::ClipboardWrite>() {
            return;
        }
        let Some(reply) = (*write).reply else {
            return;
        };
        let response = ffi::ClipboardWriteReply {
            size: std::mem::size_of::<ffi::ClipboardWriteReply>(),
            result: capture_clipboard_write(&mut *(ud as *mut Hooks), &*write),
            remember: false,
        };
        reply(write, &response);
    }
}

/// Keeps a plain-text write to the standard clipboard. A request to clear
/// the clipboard succeeds and does nothing.
///
/// # Safety
/// The request's contents and their strings must be valid for this call.
unsafe fn capture_clipboard_write(hooks: &mut Hooks, request: &ffi::ClipboardWrite) -> i32 {
    if request.location != ffi::CLIPBOARD_LOCATION_STANDARD {
        return ffi::CLIPBOARD_WRITE_UNSUPPORTED;
    }
    if request.contents_len == 0 {
        return ffi::CLIPBOARD_WRITE_SUCCESS;
    }
    if request.contents_len != 1 {
        return ffi::CLIPBOARD_WRITE_UNSUPPORTED;
    }
    if request.contents.is_null() {
        return ffi::CLIPBOARD_WRITE_INVALID_DATA;
    }
    // SAFETY: non-null, and valid for this call per the contract above.
    let content = unsafe { &*request.contents };
    // SAFETY: as above; both strings are borrowed from the live request.
    let (Some(mime), Some(bytes)) = (unsafe { borrowed_bytes(&content.mime) }, unsafe {
        borrowed_bytes(&content.data)
    }) else {
        return ffi::CLIPBOARD_WRITE_INVALID_DATA;
    };
    let is_text = std::str::from_utf8(mime)
        .ok()
        .and_then(|mime| mime.split(';').next())
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/plain"));
    if !is_text || bytes.is_empty() {
        return ffi::CLIPBOARD_WRITE_UNSUPPORTED;
    }
    if bytes.len() > CLIPBOARD_MAX_BYTES {
        return ffi::CLIPBOARD_WRITE_INVALID_DATA;
    }
    hooks.clipboard_writes.push(bytes.to_vec());
    ffi::CLIPBOARD_WRITE_SUCCESS
}

/// # Safety
/// A non-empty `value` must point at `len` bytes that outlive `'a`.
unsafe fn borrowed_bytes<'a>(value: &ffi::StringView) -> Option<&'a [u8]> {
    if value.len == 0 {
        Some(&[])
    } else if value.ptr.is_null() {
        None
    } else {
        // SAFETY: per this function's contract.
        Some(unsafe { std::slice::from_raw_parts(value.ptr, value.len) })
    }
}

pub(crate) extern "C" fn on_write_pty(
    _: ffi::Terminal,
    ud: *mut c_void,
    data: *const u8,
    len: usize,
) {
    // SAFETY: `ud` is the `Hooks` pointer installed with OPT_USERDATA,
    // valid for the terminal's life. `data` is valid for `len` bytes per
    // the write_pty callback contract. No panics cross the FFI boundary:
    // Vec growth that fails aborts, which is the intended failure mode.
    // Replies arrive as whole units; only complete units are kept, so the
    // buffer can never hold a half-written escape sequence.
    unsafe {
        let hooks = &mut *(ud as *mut Hooks);
        if hooks.replies.len() + len > REPLIES_MAX_BYTES {
            hooks.replies_overflowed = true;
            return;
        }
        hooks
            .replies
            .extend_from_slice(std::slice::from_raw_parts(data, len));
    }
}

pub(crate) extern "C" fn on_size(
    t: ffi::Terminal,
    ud: *mut c_void,
    out: *mut ffi::SizeReport,
) -> bool {
    // SAFETY: as on_write_pty; `out` is a valid report slot per contract.
    unsafe {
        let hooks = &mut *(ud as *mut Hooks);
        let (mut cols, mut rows) = (0u16, 0u16);
        let ok = ffi::ghostty_terminal_get(t, ffi::DATA_COLS, (&raw mut cols).cast())
            == ffi::SUCCESS
            && ffi::ghostty_terminal_get(t, ffi::DATA_ROWS, (&raw mut rows).cast()) == ffi::SUCCESS;
        if !ok {
            return false; // read failure: report "no size", never garbage
        }
        *out = ffi::SizeReport {
            rows,
            columns: cols,
            cell_width: hooks.cell.0,
            cell_height: hooks.cell.1,
        };
        true
    }
}

impl Drop for Vt {
    fn drop(&mut self) {
        unsafe {
            ffi::ghostty_render_state_row_cells_free(self.cells);
            ffi::ghostty_render_state_row_iterator_free(self.rows);
            ffi::ghostty_render_state_free(self.render);
            ffi::ghostty_terminal_free(self.raw);
        }
    }
}

// The owning side may move between threads; the C handles are used only
// under &mut self / &self and callbacks only fire inside those calls.
unsafe impl Send for Vt {}

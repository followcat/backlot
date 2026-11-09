// Derived from herdr-core crates/ghostty-vt/src/lib.rs (Apache-2.0).
//! Terminal ownership and the C callback hooks. Everything the C side can
//! call back into lives here; the rest of the crate sees only owned data.

use std::ffi::c_void;
use std::fmt;
use std::ptr;

use crate::Rgb;
use crate::ffi;
use crate::key::{Key, Mods, VtKeyEvent};

/// Bytes the terminal writes to the pty side (query answers, mode
/// reports) are capped: a hostile stream could otherwise grow this
/// without bound while the consumer is not looking. On overflow the
/// latch is set and whole replies are dropped — never partially kept,
/// so a caller can never see a spliced CSI/OSC/DCS fragment.
const REPLIES_MAX_BYTES: usize = 256 * 1024;

/// Cell size reported for XTWINOPS pixel queries. There is no real font;
/// 10x20 is what the core wrapper reports.
const CELL: (u32, u32) = (10, 20);

/// An operation on the terminal failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// A libghostty-vt call returned this non-success code.
    Ghostty(i32),
    /// Automatic replies exceeded the bounded buffer; whole replies were
    /// dropped. Drain more often or treat as a broken stream.
    ReplyOverflow,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Ghostty(rc) => write!(f, "libghostty-vt error {rc}"),
            Error::ReplyOverflow => {
                write!(f, "terminal reply buffer overflowed; replies were dropped")
            }
        }
    }
}

impl std::error::Error for Error {}

/// State the C callbacks receive through the userdata pointer. Owned by
/// `Vt`'s `Box`, so it outlives every callback and is never aliased
/// mutably while C could call in (callbacks only fire during `&mut self`
/// calls on `Vt`).
struct Hooks {
    /// Bytes the terminal wants written back to the pty (query replies,
    /// mode reports). Separate from input the user produces.
    replies: Vec<u8>,
    /// Set when a whole reply was dropped for capacity; cleared when the
    /// caller drains via `take_replies`.
    replies_overflowed: bool,
    /// Reported cell geometry in pixels.
    cell: (u32, u32),
    /// BEL characters seen since the last `take_bell_count`.
    bells: u64,
    /// Text programs copied since the last `take_clipboard_writes`.
    clipboard_writes: Vec<Vec<u8>>,
}

extern "C" fn on_bell(_: ffi::Terminal, ud: *mut c_void) {
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
extern "C" fn on_clipboard_write(
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

extern "C" fn on_write_pty(_: ffi::Terminal, ud: *mut c_void, data: *const u8, len: usize) {
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

extern "C" fn on_size(t: ffi::Terminal, ud: *mut c_void, out: *mut ffi::SizeReport) -> bool {
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

/// The owned terminal plus its render state and reusable iterators.
/// `!Sync` by construction (raw pointers): one owner mutates it.
pub struct Vt {
    pub(crate) raw: ffi::Terminal,
    pub(crate) cursor_tracker: crate::cursor::DecscusrTracker,
    pub(crate) render: ffi::RenderState,
    pub(crate) rows: ffi::RowIterator,
    pub(crate) cells: ffi::RowCells,
    hooks: Box<Hooks>,
    /// The overscan request currently in effect on the render state.
    pub(crate) overscan: u16,
    /// Content mutation counter — the reference's `content_seq`, used by
    /// copy mode to invalidate matches/motions computed against older
    /// buffer contents. Scroll-only changes do not count.
    content_revision: u64,
}

impl Vt {
    /// `scrollback` bounds retained scrollback bytes (0 = none).
    pub fn new(cols: u16, rows: u16, scrollback: usize) -> Vt {
        let mut raw = ptr::null_mut();
        let rc =
            unsafe { ffi::ghostty_terminal_new(ptr::null(), &mut raw, cols.max(1), rows.max(1)) };
        assert_eq!(rc, ffi::SUCCESS, "ghostty_terminal_new failed ({rc})");

        let mut render = ptr::null_mut();
        let mut row_iter = ptr::null_mut();
        let mut cells = ptr::null_mut();
        unsafe {
            let ok = ffi::ghostty_render_state_new(ptr::null(), &mut render) == ffi::SUCCESS
                && ffi::ghostty_render_state_row_iterator_new(ptr::null(), &mut row_iter)
                    == ffi::SUCCESS
                && ffi::ghostty_render_state_row_cells_new(ptr::null(), &mut cells) == ffi::SUCCESS;
            if !ok {
                if !render.is_null() {
                    ffi::ghostty_render_state_free(render);
                }
                if !row_iter.is_null() {
                    ffi::ghostty_render_state_row_iterator_free(row_iter);
                }
                if !cells.is_null() {
                    ffi::ghostty_render_state_row_cells_free(cells);
                }
                ffi::ghostty_terminal_free(raw);
                panic!("ghostty render-state allocation failed");
            }
        }

        let vt = Vt {
            raw,
            cursor_tracker: crate::cursor::DecscusrTracker::default(),
            render,
            rows: row_iter,
            cells,
            hooks: Box::new(Hooks {
                replies: Vec::new(),
                replies_overflowed: false,
                cell: CELL,
                bells: 0,
                clipboard_writes: Vec::new(),
            }),
            overscan: 0,
            content_revision: 0,
        };
        let ud: *const Hooks = &*vt.hooks;
        unsafe {
            let set = |opt, value: *const c_void| {
                let rc = ffi::ghostty_terminal_set(raw, opt, value);
                assert_eq!(
                    rc,
                    ffi::SUCCESS,
                    "ghostty_terminal_set({opt}) failed ({rc})"
                );
            };
            // Ghostty render.zig requires both defaults to be configured
            // before it reports either effective color. Match its renderer
            // fallback values internally; these are not observed host facts.
            let foreground = ffi::Rgb {
                r: 255,
                g: 255,
                b: 255,
            };
            let background = ffi::Rgb { r: 0, g: 0, b: 0 };
            set(ffi::OPT_COLOR_FOREGROUND, (&raw const foreground).cast());
            set(ffi::OPT_COLOR_BACKGROUND, (&raw const background).cast());
            set(
                ffi::OPT_SCROLLBACK_MAX_BYTES,
                (&raw const scrollback).cast(),
            );
            // VS16 emoji are two cells wide, matching upstream Ghostty.
            let cluster = ffi::ModeConfig {
                mode: 2027,
                value: true,
            };
            set(ffi::OPT_MODE_DEFAULT, (&raw const cluster).cast());
            set(ffi::OPT_USERDATA, ud.cast());
            set(
                ffi::OPT_WRITE_PTY,
                on_write_pty as ffi::WritePtyFn as *const c_void,
            );
            set(ffi::OPT_BELL, on_bell as ffi::BellFn as *const c_void);
            set(
                ffi::OPT_CLIPBOARD_WRITE,
                on_clipboard_write as ffi::ClipboardWriteFn as *const c_void,
            );
            set(ffi::OPT_SIZE, on_size as ffi::SizeFn as *const c_void);
        }
        vt
    }

    /// Feed arbitrary bytes; fragments and mid-sequence splits are fine.
    /// `ghostty_terminal_vt_write` cannot fail.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.cursor_tracker.observe(bytes);
        self.content_revision = self.content_revision.saturating_add(1);
        unsafe { ffi::ghostty_terminal_vt_write(self.raw, bytes.as_ptr(), bytes.len()) };
    }

    /// `keyboard_negotiated_nothing` from herdr src/pane/input.rs
    /// (Apache-2.0): no Kitty keyboard flags and no modifyOtherKeys2.
    /// The library exposes no flag getter, so the check is a
    /// side-effect-free Ctrl+A encode probe — `key_event` builds its
    /// encoder options from the real terminal flags, and legacy Ctrl+A
    /// is the only configuration that produces the raw `\x01` byte.
    pub fn keyboard_negotiated_nothing(&self) -> bool {
        if self.kitty_flags().unwrap_or(0) != 0 {
            return false;
        }
        let probe = VtKeyEvent {
            key: Key::Char('a'),
            mods: Mods::CTRL,
            utf8: Some("a".to_owned()),
            ..VtKeyEvent::default()
        };
        self.key_event(&probe).map(|b| b == b"\x01").unwrap_or(true)
    }

    /// Resize the grid; reports the existing cell geometry. Fails only on
    /// allocation failure inside the library.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), Error> {
        let rc = unsafe {
            ffi::ghostty_terminal_resize(
                self.raw,
                cols.max(1),
                rows.max(1),
                self.hooks.cell.0,
                self.hooks.cell.1,
            )
        };
        (rc == ffi::SUCCESS)
            .then(|| self.content_revision = self.content_revision.saturating_add(1))
            .ok_or(Error::Ghostty(rc))
    }

    /// The content mutation counter — bumps on every `feed` and `resize`.
    /// Copy mode snapshots it to notice buffer changes under it.
    pub fn content_revision(&self) -> u64 {
        self.content_revision
    }

    pub fn size(&self) -> Result<(u16, u16), Error> {
        let (mut cols, mut rows) = (0u16, 0u16);
        unsafe {
            ok(ffi::ghostty_terminal_get(
                self.raw,
                ffi::DATA_COLS,
                (&raw mut cols).cast(),
            ))?;
            ok(ffi::ghostty_terminal_get(
                self.raw,
                ffi::DATA_ROWS,
                (&raw mut rows).cast(),
            ))?;
        }
        Ok((cols, rows))
    }

    /// A DEC private mode (e.g. 2004 bracketed paste).
    pub fn mode(&self, mode: u16) -> Result<bool, Error> {
        let mut cfg = ffi::ModeConfig {
            mode: mode & 0x7FFF,
            value: false,
        };
        let rc =
            unsafe { ffi::ghostty_terminal_get(self.raw, ffi::DATA_MODE, (&raw mut cfg).cast()) };
        ok(rc)?;
        Ok(cfg.value)
    }

    /// True while the program uses the alternate screen.
    pub fn alt_screen(&self) -> Result<bool, Error> {
        let mut screen: i32 = 0;
        let rc = unsafe {
            ffi::ghostty_terminal_get(self.raw, ffi::DATA_ACTIVE_SCREEN, (&raw mut screen).cast())
        };
        ok(rc)?;
        Ok(screen == 1)
    }

    /// True while the program has any mouse tracking mode enabled —
    /// wheel/button input should be reported to it as SGR sequences
    /// instead of moving the viewport.
    pub fn mouse_tracking(&self) -> bool {
        let mut tracking = false;
        let rc = unsafe {
            ffi::ghostty_terminal_get(
                self.raw,
                ffi::DATA_MOUSE_TRACKING,
                (&raw mut tracking).cast(),
            )
        };
        rc == ffi::SUCCESS && tracking
    }

    /// The viewport's scrollbar state `{total, offset, len}` — `offset`
    /// counts rows from the top of the scrollable area, so "scrolled
    /// back lines" is `total - offset - len`.
    pub fn scrollbar(&self) -> Option<ffi::Scrollbar> {
        let mut bar = ffi::Scrollbar::default();
        let rc = unsafe {
            ffi::ghostty_terminal_get(self.raw, ffi::DATA_SCROLLBAR, (&raw mut bar).cast())
        };
        (rc == ffi::SUCCESS).then_some(bar)
    }

    /// Scrolls the viewport by `delta` rows — negative up (toward
    /// scrollback), positive down. The VT clamps to its scrollable
    /// area; alt-screen programs report no scrollback so this no-ops.
    pub fn scroll_delta(&mut self, delta: isize) {
        unsafe {
            ffi::ghostty_terminal_scroll_viewport(
                self.raw,
                ffi::ScrollViewport {
                    tag: ffi::SCROLL_DELTA,
                    value: ffi::ScrollViewportValue { delta },
                },
            );
        }
    }

    /// Scrolls the viewport to an absolute row offset (`scrollbar`'s
    /// row space) — scrollbar drag/jump target.
    pub fn scroll_to_row(&mut self, row: usize) {
        unsafe {
            ffi::ghostty_terminal_scroll_viewport(
                self.raw,
                ffi::ScrollViewport {
                    tag: ffi::SCROLL_ROW,
                    value: ffi::ScrollViewportValue { row },
                },
            );
        }
    }

    /// Seed the terminal's default colors with the host terminal's
    /// answers (`OSC 10`/`OSC 11`/`OSC 4` replies): `CellColor::Default`
    /// then resolves against the host palette instead of the built-in
    /// defaults, so unchanged defaults stay transparent over the host's
    /// own background. `None` parts keep the built-in defaults; a `None`
    /// palette leaves the built-in 256 entries. Any palette entries the
    /// program later overrides via `OSC 4` still take precedence over
    /// the seeded defaults.
    pub fn set_default_colors(
        &mut self,
        foreground: Option<Rgb>,
        background: Option<Rgb>,
        palette: Option<&[Rgb; 256]>,
    ) -> Result<(), Error> {
        unsafe {
            if let Some(fg) = foreground {
                let c = ffi::Rgb::from(fg);
                ok(ffi::ghostty_terminal_set(
                    self.raw,
                    ffi::OPT_COLOR_FOREGROUND,
                    (&raw const c).cast(),
                ))?;
            }
            if let Some(bg) = background {
                let c = ffi::Rgb::from(bg);
                ok(ffi::ghostty_terminal_set(
                    self.raw,
                    ffi::OPT_COLOR_BACKGROUND,
                    (&raw const c).cast(),
                ))?;
            }
            if let Some(palette) = palette {
                let raw: [ffi::Rgb; 256] = palette.map(ffi::Rgb::from);
                ok(ffi::ghostty_terminal_set(
                    self.raw,
                    ffi::OPT_COLOR_PALETTE,
                    (&raw const raw).cast(),
                ))?;
            }
        }
        Ok(())
    }

    /// The pixel cell pitch reported for XTWINOPS cell-size queries —
    /// the host's truthful pitch, or the fallback when unknown.
    pub fn set_cell_px(&mut self, width: u32, height: u32) {
        self.hooks.cell = (width.max(1), height.max(1));
    }

    /// Current pixel pitch, measured by the host or the VT fallback.
    pub fn cell_px(&self) -> (u32, u32) {
        self.hooks.cell
    }

    /// BEL characters the terminal rang since the last drain — the
    /// ghostty `OPT_BELL` hook the reference uses for pane bells.
    pub fn take_bell_count(&mut self) -> u64 {
        std::mem::take(&mut self.hooks.bells)
    }

    /// Text programs copied (OSC 52 to the standard clipboard) since the
    /// last drain, oldest first and already decoded.
    pub fn take_clipboard_writes(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.hooks.clipboard_writes)
    }

    /// Bytes the terminal generated for the pty side — query replies and
    /// mode reports, never user input. Send via `Attach::automatic_input`.
    /// `ReplyOverflow` reports that whole replies were dropped and the
    /// buffer is cleared; the caller decides whether the stream is still
    /// trustworthy.
    pub fn take_replies(&mut self) -> Result<Vec<u8>, Error> {
        if self.hooks.replies_overflowed {
            self.hooks.replies_overflowed = false;
            self.hooks.replies.clear();
            return Err(Error::ReplyOverflow);
        }
        Ok(std::mem::take(&mut self.hooks.replies))
    }
}

fn ok(rc: i32) -> Result<(), Error> {
    (rc == ffi::SUCCESS).then_some(()).ok_or(Error::Ghostty(rc))
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

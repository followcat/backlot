// Derived from herdr-core crates/ghostty-vt/src/lib.rs (Apache-2.0).
//! Terminal ownership and the C callback hooks. Everything the C side can
//! call back into lives here; the rest of the crate sees only owned data.

use std::ffi::c_void;
use std::fmt;
use std::ptr;

use crate::ffi;

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
    pub(crate) render: ffi::RenderState,
    pub(crate) rows: ffi::RowIterator,
    pub(crate) cells: ffi::RowCells,
    hooks: Box<Hooks>,
    /// The overscan request currently in effect on the render state.
    pub(crate) overscan: u16,
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
            render,
            rows: row_iter,
            cells,
            hooks: Box::new(Hooks {
                replies: Vec::new(),
                replies_overflowed: false,
                cell: CELL,
            }),
            overscan: 0,
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
            set(ffi::OPT_SIZE, on_size as ffi::SizeFn as *const c_void);
        }
        vt
    }

    /// Feed arbitrary bytes; fragments and mid-sequence splits are fine.
    /// `ghostty_terminal_vt_write` cannot fail.
    pub fn feed(&mut self, bytes: &[u8]) {
        unsafe { ffi::ghostty_terminal_vt_write(self.raw, bytes.as_ptr(), bytes.len()) };
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
        (rc == ffi::SUCCESS).then_some(()).ok_or(Error::Ghostty(rc))
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

// Derived from herdr-core crates/ghostty-vt/src/lib.rs (Apache-2.0).
//! Terminal ownership: construction, feeding, options and queries. The
//! C callback hooks live in `hooks.rs`; the rest of the crate sees only
//! owned data.

use std::ffi::c_void;
use std::fmt;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::Rgb;
use crate::ffi;
use crate::hooks::{Hooks, on_bell, on_clipboard_write, on_size, on_write_pty};
use crate::key::{Key, Mods, VtKeyEvent};

/// Cell size reported for XTWINOPS pixel queries. There is no real font;
/// 10x20 is what the core wrapper reports.
const CELL: (u32, u32) = (10, 20);

/// Identity source for `Vt::id` — each construction takes a fresh value
/// so a pane cache can tell "same VT, quiet" from "new VT, repaint"
/// without relying on the render address (a freed handle can repeat).
static NEXT_VT_ID: AtomicU64 = AtomicU64::new(1);

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

/// The owned terminal plus its render state and reusable iterators.
/// `!Sync` by construction (raw pointers): one owner mutates it.
pub struct Vt {
    /// Unique identity — assigned from `NEXT_VT_ID` at construction.
    /// Cache holders compare it instead of a handle address so an
    /// ABA reuse of a freed render pointer can never pass for "same VT".
    id: u64,
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
            id: NEXT_VT_ID.fetch_add(1, Ordering::Relaxed),
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

    /// This terminal's unique identity — a cache keyed on it can never
    /// confuse a replacement `Vt` for the one its cells came from.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Feed arbitrary bytes; mid-sequence splits are fine.
    /// `ghostty_terminal_vt_write` cannot fail.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.cursor_tracker.observe(bytes);
        self.content_revision = self.content_revision.saturating_add(1);
        unsafe { ffi::ghostty_terminal_vt_write(self.raw, bytes.as_ptr(), bytes.len()) };
    }

    /// `keyboard_negotiated_nothing` from herdr src/pane/input.rs
    /// (Apache-2.0): no Kitty flags and no modifyOtherKeys2. The library
    /// exposes no flag getter, so a Ctrl+A encode probe answers it —
    /// legacy encoding is the only mode producing the raw `\x01` byte.
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

    /// Resize the grid; reports the existing cell geometry.
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

    /// The content mutation counter — bumps on `feed` and `resize`.
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

    /// The configured scrollback byte cap — the value `Vt::new` was
    /// built with, reported back by the library. `None` on unlimited
    /// (or query failure); the library enforces at page granularity,
    /// so this is the configured bound, not the effective figure.
    pub fn scrollback_max_bytes(&self) -> Option<usize> {
        let mut out: usize = 0;
        let rc = unsafe {
            ffi::ghostty_terminal_get(
                self.raw,
                ffi::DATA_SCROLLBACK_MAX_BYTES,
                (&raw mut out).cast(),
            )
        };
        (rc == ffi::SUCCESS).then_some(out)
    }

    /// Scrollbar state `{total, offset, len}` — "scrolled back lines"
    /// is `total - offset - len`.
    pub fn scrollbar(&self) -> Option<ffi::Scrollbar> {
        let mut bar = ffi::Scrollbar::default();
        let rc = unsafe {
            ffi::ghostty_terminal_get(self.raw, ffi::DATA_SCROLLBAR, (&raw mut bar).cast())
        };
        (rc == ffi::SUCCESS).then_some(bar)
    }

    /// Scroll the viewport by `delta` rows — negative up (toward
    /// scrollback). The VT clamps; alt-screen has no scrollback.
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

    /// Scroll the viewport to an absolute `scrollbar` row offset.
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

    /// Seed default colors with the host terminal's `OSC 10`/`OSC 11`/
    /// `OSC 4` answers so `CellColor::Default` resolves against the
    /// host palette. `None` parts keep the built-ins.
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
    /// mode reports, never user input. `ReplyOverflow` means replies
    /// were dropped and the buffer cleared.
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

//! Outer-terminal lifecycle: raw mode, alternate screen, and the
//! keyboard/paste/focus modes the session needs — plus their restore.
//! `TermGuard` restores on drop, so a panic or early error path cannot
//! leave the user's terminal half-configured; the outer cursor style
//! the app set is restored too. No bytes the inner VT produced ever
//! reach the outer terminal from here.

use std::io::{self, Write};

use crossterm::cursor::{SetCursorStyle, Show};
use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    DisableLineWrap, EnableLineWrap, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
    enable_raw_mode,
};

/// What the event reader asks for: disambiguated keys (press/repeat/
/// release) so kitty-reporting terminals and the prefix machine get a
/// faithful event stream.
const KEY_FLAGS: KeyboardEnhancementFlags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
    .union(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
    .union(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS);

/// `HOST_COLOR_SCHEME_REPORT_ENABLE_SEQUENCE` — host pushes `?997;Nn`
/// when its dark/light scheme flips; the framer re-queries colors.
const SCHEME_REPORT_ON: &str = "\x1b[?2031h";
/// `HOST_COLOR_SCHEME_REPORT_DISABLE_SEQUENCE` — restore pair.
const SCHEME_REPORT_OFF: &str = "\x1b[?2031l";
/// modifyOtherKeys reset — `CSI >4;0m` undoes any `>4;Nm` we set.
const MODIFY_OTHER_KEYS_OFF: &str = "\x1b[>4;0m";

/// An owning terminal setup: every mode flipped here is undone in
/// `Drop`, in reverse order, regardless of how the app ended.
pub struct TermGuard {
    out: io::Stdout,
    /// Whether `PushKeyboardEnhancementFlags` succeeded — `Pop` is
    /// sent whenever the push did (including a failed enter that got
    /// that far).
    keyboard: bool,
    /// The host's cooked-mode VERASE, read before raw mode — a `^H`
    /// erase makes a raw 0x08 the Backspace key (#3244).
    erase: Option<u8>,
    /// `?2031h` was written — restore needs `?2031l`.
    scheme_reports: bool,
    /// The modifyOtherKeys sequence written at setup (`>4;1m` or
    /// `>4;2m`), `None` when the env probe said no — restore then
    /// skips `>4;0m` the way the reference does.
    modify_other_keys: Option<&'static str>,
}

impl TermGuard {
    /// Enters the alternate screen in raw mode with paste/focus
    /// reporting and — where the terminal honors it — key event
    /// reporting. Any failure restores whatever was already flipped,
    /// including a succeeded keyboard push. `mouse_capture` mirrors the
    /// reference's `set_mouse_capture`: every known host mouse mode is
    /// cleared first, then SGR button-reporting is enabled only when
    /// the config asks for it.
    pub fn enter(mouse_capture: bool) -> io::Result<TermGuard> {
        let mut out = io::stdout();
        let mut keyboard = false;
        let erase = host_erase_byte();
        let result = (|| {
            enable_raw_mode()?;
            execute!(out, EnterAlternateScreen)?;
            crate::terminal_modes::clear_host_mouse_reporting(&mut out)?;
            if mouse_capture {
                execute!(out, EnableMouseCapture)?;
            }
            execute!(out, EnableBracketedPaste, EnableFocusChange)?;
            keyboard = execute!(out, PushKeyboardEnhancementFlags(KEY_FLAGS)).is_ok();
            out.write_all(SCHEME_REPORT_ON.as_bytes())?;
            let mok = host_modify_other_keys_mode();
            if let Some(seq) = mok {
                out.write_all(seq.as_bytes())?;
            }
            execute!(out, DisableLineWrap)?;
            out.flush().map(|()| mok)
        })();
        match result {
            Ok(mok) => Ok(TermGuard {
                out,
                keyboard,
                erase,
                scheme_reports: true,
                modify_other_keys: mok,
            }),
            Err(e) => {
                // Best-effort restore of whatever succeeded — the same
                // work Drop performs. The `l`/`0` resets are inert when
                // the matching enable never ran.
                let _ = out.write_all(SCHEME_REPORT_OFF.as_bytes());
                let _ = out.write_all(MODIFY_OTHER_KEYS_OFF.as_bytes());
                if keyboard {
                    let _ = execute!(out, PopKeyboardEnhancementFlags);
                }
                let _ = execute!(
                    out,
                    EnableLineWrap,
                    DisableBracketedPaste,
                    DisableFocusChange
                );
                let _ = execute!(out, DisableMouseCapture);
                let _ = crate::terminal_modes::clear_host_mouse_reporting(&mut out);
                let _ = execute!(out, LeaveAlternateScreen);
                let _ = disable_raw_mode();
                Err(e)
            }
        }
    }

    /// Underlying writer for the ratatui backend.
    pub fn writer(&mut self) -> &mut io::Stdout {
        &mut self.out
    }

    /// The host's cooked-mode erase byte (`None` off-Unix).
    pub fn erase_byte(&self) -> Option<u8> {
        self.erase
    }
}

/// The host terminal's VERASE — must read the termios before raw
/// mode, so `enter` captures it first. `tcgetattr` failure or a
/// non-Unix host maps to `None`; the rewrite stays inert.
#[cfg(unix)]
fn host_erase_byte() -> Option<u8> {
    unsafe {
        let mut termios = std::mem::zeroed::<libc::termios>();
        (libc::tcgetattr(libc::STDIN_FILENO, &mut termios) == 0).then(|| termios.c_cc[libc::VERASE])
    }
}

#[cfg(not(unix))]
fn host_erase_byte() -> Option<u8> {
    None
}

/// The reference's `host_modify_other_keys_mode` env probe: tmux needs
/// mode 2 (`>4;2m`), WezTerm/Alacritty take mode 1, anything else gets
/// no modifyOtherKeys — it would double-encode keys kitty already
/// reports.
fn host_modify_other_keys_mode() -> Option<&'static str> {
    if std::env::var_os("TMUX").is_some() {
        return Some("\x1b[>4;2m");
    }
    if std::env::var_os("WEZTERM_PANE").is_some()
        || std::env::var_os("ALACRITTY_WINDOW_ID").is_some()
        || std::env::var("TERM_PROGRAM")
            .map(|p| p.eq_ignore_ascii_case("wezterm"))
            .unwrap_or(false)
    {
        return Some("\x1b[>4;1m");
    }
    None
}

impl Drop for TermGuard {
    fn drop(&mut self) {
        // Restore the outer cursor first: visibility and the user's
        // own shape, whatever the inner program asked for.
        let _ = execute!(self.out, Show, SetCursorStyle::DefaultUserShape);
        if self.modify_other_keys.is_some() {
            let _ = self.out.write_all(MODIFY_OTHER_KEYS_OFF.as_bytes());
        }
        if self.keyboard {
            let _ = execute!(self.out, PopKeyboardEnhancementFlags);
        }
        let _ = execute!(
            self.out,
            EnableLineWrap,
            DisableBracketedPaste,
            DisableFocusChange
        );
        let _ = execute!(self.out, DisableMouseCapture);
        let _ = crate::terminal_modes::clear_host_mouse_reporting(&mut self.out);
        if self.scheme_reports {
            let _ = self.out.write_all(SCHEME_REPORT_OFF.as_bytes());
        }
        let _ = execute!(self.out, LeaveAlternateScreen);
        let _ = disable_raw_mode();
        let _ = self.out.flush();
    }
}

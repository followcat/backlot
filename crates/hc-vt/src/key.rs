// Derived from herdr-core crates/ghostty-vt/src/lib.rs and
// herdr src/pane/input.rs (Apache-2.0).
//! Neutral key and paste input encoded against the terminal's modes:
//! application cursor/keypad, modifyOtherKeys and the negotiated Kitty
//! keyboard flags all come from the live terminal. No Windows-specific
//! translator — win32 input events are outside v1 scope.

use std::ptr;

use crate::ffi;
use crate::vt::{Error, Vt};

/// A keyboard event the UI wants to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInput {
    pub key: Key,
    pub mods: Mods,
    pub action: Action,
}

/// Keys the UI can name. `Char` is text input: ASCII maps to the physical
/// key and its unshifted codepoint (shifted punctuation to the base key),
/// any other scalar goes as `GHOSTTY_KEY_UNIDENTIFIED` with its UTF-8
/// text — under Kitty keyboard the program sees the codepoint, otherwise
/// the text lands as typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Enter,
    Tab,
    /// Shift-Tab — encoded as Tab+Shift like upstream.
    BackTab,
    Backspace,
    Escape,
    Space,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    /// F1..=F25.
    F(u8),
    /// Named non-text Kitty key, mapped through the pinned native key table.
    Functional(u32),
}

/// Modifier bits (GhosttyMods values for SHIFT/CTRL/ALT/SUPER; HYPER and
/// META live in the kitty modifier mask but have no Ghostty equivalent —
/// they stay in high bits so callers can see and drop them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods(pub u16);

impl Mods {
    pub const NONE: Mods = Mods(0);
    pub const SHIFT: Mods = Mods(ffi::MOD_SHIFT);
    pub const CTRL: Mods = Mods(ffi::MOD_CTRL);
    pub const ALT: Mods = Mods(ffi::MOD_ALT);
    pub const SUPER: Mods = Mods(ffi::MOD_SUPER);
    pub const HYPER: Mods = Mods(0x400);
    pub const META: Mods = Mods(0x800);

    pub fn union(self, other: Mods) -> Mods {
        Mods(self.0 | other.0)
    }

    pub fn contains(self, other: Mods) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn intersects(self, other: Mods) -> bool {
        self.0 & other.0 != 0
    }

    /// `self & other` (crossterm `KeyModifiers` intersection).
    pub fn intersection(self, other: Mods) -> Mods {
        Mods(self.0 & other.0)
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// `self - other` (crossterm `KeyModifiers` subtraction).
    pub fn difference(self, other: Mods) -> Mods {
        Mods(self.0 & !other.0)
    }

    /// Only the bits libghostty knows — HYPER/META are stripped.
    pub(crate) fn ghostty(self) -> u16 {
        self.0 & (Mods::SHIFT.0 | Mods::CTRL.0 | Mods::ALT.0 | Mods::SUPER.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Action {
    #[default]
    Press,
    Repeat,
    Release,
}

/// RAII for the two C handles `key` needs — freed on every exit path.
struct Encoder(ffi::KeyEncoder);
struct KeyEvent(ffi::KeyEvent);

impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe { ffi::ghostty_key_encoder_free(self.0) }
    }
}

impl Drop for KeyEvent {
    fn drop(&mut self) {
        unsafe { ffi::ghostty_key_event_free(self.0) }
    }
}

// GhosttyKey ordinals for the keys we expose (vendor/ghostty
// include/ghostty/vt/key/event.h, pinned — verified by the encode tests).
mod keys {
    pub const UNIDENTIFIED: i32 = 0;
    pub const BACKQUOTE: i32 = 1;
    pub const BACKSLASH: i32 = 2;
    pub const BRACKET_LEFT: i32 = 3;
    pub const BRACKET_RIGHT: i32 = 4;
    pub const COMMA: i32 = 5;
    pub const DIGIT_0: i32 = 6;
    pub const EQUAL: i32 = 16;
    pub const A: i32 = 20;
    pub const MINUS: i32 = 46;
    pub const PERIOD: i32 = 47;
    pub const QUOTE: i32 = 48;
    pub const SEMICOLON: i32 = 49;
    pub const SLASH: i32 = 50;
    pub const BACKSPACE: i32 = 53;
    pub const ENTER: i32 = 58;
    pub const SPACE: i32 = 63;
    pub const TAB: i32 = 64;
    pub const DELETE: i32 = 68;
    pub const END: i32 = 69;
    pub const HOME: i32 = 71;
    pub const INSERT: i32 = 72;
    pub const PAGE_DOWN: i32 = 73;
    pub const PAGE_UP: i32 = 74;
    pub const ARROW_DOWN: i32 = 75;
    pub const ARROW_LEFT: i32 = 76;
    pub const ARROW_RIGHT: i32 = 77;
    pub const ARROW_UP: i32 = 78;
    pub const ESCAPE: i32 = 120;
    pub const F1: i32 = 121;
}

/// The physical key an ASCII character lives on, plus the codepoint that
/// key produces unshifted — what the Kitty `report alternates` flag and
/// the legacy encoders need. Non-ASCII text has no physical key.
fn physical(c: char) -> (i32, u32) {
    // Shifted punctuation -> the unshifted base key it lives on.
    let unshifted = match c {
        '!' => '1',
        '@' => '2',
        '#' => '3',
        '$' => '4',
        '%' => '5',
        '^' => '6',
        '&' => '7',
        '*' => '8',
        '(' => '9',
        ')' => '0',
        '_' => '-',
        '+' => '=',
        '{' => '[',
        '}' => ']',
        '|' => '\\',
        ':' => ';',
        '"' => '\'',
        '<' => ',',
        '>' => '.',
        '?' => '/',
        '~' => '`',
        _ => c.to_ascii_lowercase(),
    };
    (named_char_key(unshifted), unshifted as u32)
}

/// The reference `ghostty_key_from_char`: only a reported base key
/// names a physical key. A layout's `+` is not assumed to live on `=`.
fn named_char_key(base: char) -> i32 {
    use keys::*;
    match base {
        'a'..='z' => A + i32::from(base as u8 - b'a'),
        '0'..='9' => DIGIT_0 + i32::from(base as u8 - b'0'),
        '`' => BACKQUOTE,
        '\\' => BACKSLASH,
        '[' => BRACKET_LEFT,
        ']' => BRACKET_RIGHT,
        ',' => COMMA,
        '=' => EQUAL,
        '-' => MINUS,
        '.' => PERIOD,
        '\'' => QUOTE,
        ';' => SEMICOLON,
        '/' => SLASH,
        ' ' => SPACE,
        _ => UNIDENTIFIED,
    }
}

fn ghostty_key(key: Key) -> i32 {
    use keys::*;
    match key {
        Key::Char(c) => named_char_key(c),
        Key::Enter => ENTER,
        Key::Tab | Key::BackTab => TAB,
        Key::Backspace => BACKSPACE,
        Key::Escape => ESCAPE,
        Key::Space => SPACE,
        Key::Delete => DELETE,
        Key::Insert => INSERT,
        Key::Home => HOME,
        Key::End => END,
        Key::PageUp => PAGE_UP,
        Key::PageDown => PAGE_DOWN,
        Key::ArrowUp => ARROW_UP,
        Key::ArrowDown => ARROW_DOWN,
        Key::ArrowLeft => ARROW_LEFT,
        Key::ArrowRight => ARROW_RIGHT,
        Key::F(n) => F1 + i32::from(n.clamp(1, 25)) - 1,
        // Derived from herdr src/pane/input.rs (Apache-2.0), ordinals
        // verified against pinned include/ghostty/vt/key/event.h.
        Key::Functional(code) => match code {
            57358 => 54,
            57359 => 149,
            57360 => 79,
            57361 => 148,
            57362 => 150,
            57363 => 55,
            57427 => 113,
            57430 => 162,
            57432 => 164,
            57435 => 165,
            57436 => 166,
            57438 => 169,
            57439 => 171,
            57440 => 170,
            _ => UNIDENTIFIED,
        },
    }
}

/// The full key-event fields `ghostty_key_event_from_terminal_key` sets:
/// the physical key, ghostty-space modifiers, consumed modifiers (Shift
/// consumed producing text), the unshifted codepoint and the utf8 text.
#[derive(Debug, Clone)]
pub struct VtKeyEvent {
    pub action: Action,
    /// The reported base character or named key. Shifted punctuation
    /// is not translated to a guessed US base key; the caller supplies
    /// the actual base and layout text, as the reference builder does.
    pub key: Key,
    pub mods: Mods,
    /// Modifiers the text already consumed (MOD_SHIFT when Shift made
    /// the reported character).
    pub consumed_mods: Mods,
    /// The codepoint the key produces without modifiers; `None` leaves
    /// the library default (the physical key's own codepoint).
    pub unshifted_codepoint: Option<u32>,
    /// The text the press produces; `None` = no text.
    pub utf8: Option<String>,
    /// IME/dead-key composition in progress.
    pub composing: bool,
}

impl Default for VtKeyEvent {
    fn default() -> Self {
        VtKeyEvent {
            action: Action::Press,
            key: Key::Char('\0'),
            mods: Mods::NONE,
            consumed_mods: Mods::NONE,
            unshifted_codepoint: None,
            utf8: None,
            composing: false,
        }
    }
}

impl Vt {
    /// The pane's negotiated Kitty keyboard flags (0 = legacy pane).
    pub fn kitty_flags(&self) -> Result<u8, Error> {
        let mut flags: u8 = 0;
        let rc = unsafe {
            ffi::ghostty_terminal_get(
                self.raw,
                ffi::DATA_KITTY_KEYBOARD_FLAGS,
                (&raw mut flags).cast(),
            )
        };
        ok(rc)?;
        Ok(flags)
    }

    /// Encode a key event against the terminal's current modes. The
    /// result is human input: send it via the input path, never
    /// `automatic_input`. An empty `Ok` means the event legitimately
    /// produces no bytes (e.g. a release with event reporting off).
    pub fn key(&self, input: &KeyInput) -> Result<Vec<u8>, Error> {
        let mut key = input.key;
        let mut text = None;
        let mut unshifted_codepoint = None;
        if let Key::Char(c) = input.key {
            let (_, base) = physical(c);
            key = Key::Char(char::from_u32(base).unwrap_or(c));
            text = Some(c.to_string());
            unshifted_codepoint = Some(base);
        }
        self.key_event(&VtKeyEvent {
            action: input.action,
            key,
            mods: input.mods,
            unshifted_codepoint,
            utf8: text,
            ..VtKeyEvent::default()
        })
    }

    /// Field-level encode: the caller built the event exactly the way
    /// `ghostty_key_event_from_terminal_key` builds it.
    pub fn key_event(&self, spec: &VtKeyEvent) -> Result<Vec<u8>, Error> {
        unsafe {
            let mut e = ptr::null_mut();
            ok(ffi::ghostty_key_encoder_new(ptr::null(), &mut e))?;
            let encoder = Encoder(e);
            let mut v = ptr::null_mut();
            ok(ffi::ghostty_key_event_new(ptr::null(), &mut v))?;
            let event = KeyEvent(v);

            ffi::ghostty_key_encoder_setopt_from_terminal(encoder.0, self.raw);
            // These are named keys, not a Mac keyboard: alt always means
            // alt — from_terminal resets this to false.
            let option_as_alt: i32 = ffi::OPTION_AS_ALT_TRUE;
            ffi::ghostty_key_encoder_setopt(
                encoder.0,
                ffi::ENC_MACOS_OPTION_AS_ALT,
                (&raw const option_as_alt).cast(),
            );
            ffi::ghostty_key_event_set_action(
                event.0,
                match spec.action {
                    Action::Press => ffi::KEY_PRESS,
                    Action::Repeat => ffi::KEY_REPEAT,
                    Action::Release => ffi::KEY_RELEASE,
                },
            );
            ffi::ghostty_key_event_set_key(event.0, ghostty_key(spec.key));
            ffi::ghostty_key_event_set_mods(event.0, spec.mods.ghostty());
            if spec.consumed_mods.0 != 0 {
                ffi::ghostty_key_event_set_consumed_mods(event.0, spec.consumed_mods.ghostty());
            }
            if let Some(codepoint) = spec.unshifted_codepoint {
                ffi::ghostty_key_event_set_unshifted_codepoint(event.0, codepoint);
            }
            if let Some(text) = &spec.utf8 {
                ffi::ghostty_key_event_set_utf8(event.0, text.as_ptr(), text.len());
            }
            if spec.composing {
                ffi::ghostty_key_event_set_composing(event.0, true);
            }
            encode(encoder.0, event.0)
        }
    }

    /// Encode pasted text. Wrapping follows the terminal's bracketed
    /// paste mode exactly once; unsafe control bytes are stripped by the
    /// library. Result is user input, like `key`.
    pub fn paste(&self, text: &[u8]) -> Result<Vec<u8>, Error> {
        let bracketed = self.mode(2004)?;
        // The encoder modifies `data` in place: hand it a fresh copy for
        // each call so a failed size probe cannot leave half-rewritten
        // bytes feeding the real encode.
        let mut written: usize = 0;
        let rc = unsafe {
            ffi::ghostty_paste_encode(
                text.to_vec().as_mut_ptr(),
                text.len(),
                bracketed,
                ptr::null_mut(),
                0,
                &mut written,
            )
        };
        if rc == ffi::SUCCESS && written == 0 {
            return Ok(Vec::new());
        }
        if rc != ffi::OUT_OF_SPACE {
            return Err(Error::Ghostty(rc));
        }
        let mut out = vec![0u8; written];
        let rc = unsafe {
            ffi::ghostty_paste_encode(
                text.to_vec().as_mut_ptr(),
                text.len(),
                bracketed,
                out.as_mut_ptr(),
                out.len(),
                &mut written,
            )
        };
        ok(rc)?;
        out.truncate(written);
        Ok(out)
    }
}

/// Size-query then encode — Kitty sequences can exceed any fixed buffer.
fn encode(encoder: ffi::KeyEncoder, event: ffi::KeyEvent) -> Result<Vec<u8>, Error> {
    unsafe {
        let mut written: usize = 0;
        let rc = ffi::ghostty_key_encoder_encode(encoder, event, ptr::null_mut(), 0, &mut written);
        if rc == ffi::SUCCESS {
            return Ok(Vec::new()); // event produces no bytes
        }
        if rc != ffi::OUT_OF_SPACE {
            return Err(Error::Ghostty(rc));
        }
        let mut out = vec![0u8; written];
        let rc = ffi::ghostty_key_encoder_encode(
            encoder,
            event,
            out.as_mut_ptr(),
            out.len(),
            &mut written,
        );
        ok(rc)?;
        out.truncate(written);
        Ok(out)
    }
}

fn ok(rc: i32) -> Result<(), Error> {
    (rc == ffi::SUCCESS).then_some(()).ok_or(Error::Ghostty(rc))
}

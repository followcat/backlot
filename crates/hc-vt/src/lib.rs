//! `hc-vt`: a narrow, safe adapter over libghostty-vt for the TUI's
//! terminal panes.
//!
//! One [`Vt`] owns one `GhosttyTerminal` and its render state. Feed it
//! arbitrary pty bytes with [`Vt::feed`], take owned snapshots with
//! [`Vt::frame`], encode neutral key/paste input against the terminal's
//! modes with [`Vt::key`]/[`Vt::paste`], and read VT-generated replies
//! (query answers the terminal itself writes to the pty) via
//! [`Vt::take_replies`].
//!
//! Ownership and safety rules the adapter enforces:
//! - `Vt` is `Send` but not `Sync`: exactly one owner mutates it.
//! - Frame data is copied out of the render state; no snapshot holds
//!   pointers into C memory, so reads stay valid after later mutation.
//! - Grapheme clusters are sized by the library per cell — no fixed
//!   codepoint buffer.
//! - Automatic replies stay separate from human input. Clipboard writes
//!   go to the caller through [`Vt::take_clipboard_writes`]; nothing
//!   else the terminal emits is forwarded to the host terminal.
//!
//! Not here: sidebar state, status detection (the core API is the only
//! source), selection/copy UX, and win32 input translation.

mod cursor;
mod data;
mod ffi;
mod key;
mod mouse;
mod render;
mod screen_text;
mod text;
mod vt;

pub use data::{
    Cell, CellColor, Colors, Cursor, CursorShape, Dirty, Frame, Rgb, Row, Underline, Wide,
};
pub use key::{Action, Key, KeyInput, Mods, VtKeyEvent};
pub use mouse::{MouseAction, MouseButton, MouseInput, MousePosition};
pub use screen_text::{KITTY_UNICODE_PLACEHOLDER, ScreenTextCell, ScreenTextRow};
pub use text::codepoint_width;
pub use vt::{Error, Vt};

mod appearance;
mod title;
pub use appearance::Appearance;

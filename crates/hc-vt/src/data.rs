//! Owned render data: the types `Vt::frame` fills. Everything here is a
//! plain Rust value — no pointers into C memory, so a `Frame` stays valid
//! while the `Vt` keeps changing.

use crate::ffi;
use crate::vt::Error;

/// One rendered frame. `rows` are ordered top to bottom; overscan rows
/// above the viewport carry negative `y`, viewport rows `0..rows-1`.
#[derive(Debug, Clone)]
pub struct Frame {
    pub cols: u16,
    pub rows: Vec<Row>,
    pub cursor: Cursor,
    pub colors: Colors,
    pub dirty: Dirty,
}

#[derive(Debug, Clone)]
pub struct Row {
    /// Position relative to the top of the viewport (negative = scrollback).
    pub y: i32,
    pub dirty: bool,
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone)]
pub struct Cell {
    /// Full grapheme cluster as UTF-8; empty for spacer tails and blanks.
    pub text: String,
    pub wide: Wide,
    /// Colors as the program set them; resolve through `Colors::resolve`.
    /// A cell carrying a packed (erase-fill) background reports it as
    /// `Rgb` even though its style carries no color.
    pub fg: CellColor,
    pub bg: CellColor,
    pub underline_color: CellColor,
    pub underline: Underline,
    pub bold: bool,
    pub faint: bool,
    pub italic: bool,
    pub inverse: bool,
    pub invisible: bool,
    pub strikethrough: bool,
    pub overline: bool,
    pub blink: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wide {
    Narrow,
    Wide,
    /// Second cell of a wide grapheme: paint nothing, it is covered.
    SpacerTail,
    SpacerHead,
}

/// A color as written by the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellColor {
    Default,
    Palette(u8),
    Rgb(Rgb),
}

/// A resolved RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

#[derive(Debug, Clone, Copy)]
pub struct Cursor {
    /// `None` when the cursor is outside the viewport (scrolled back).
    pub position: Option<(u16, u16)>,
    pub visible: bool,
    pub blinking: bool,
    pub password_input: bool,
    pub shape: CursorShape,
    pub wide_tail: bool,
    /// Source policy: DECSCUSR 0 until an explicit shape is observed.
    pub shape_overridden: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    /// DECSCUSR 0: retain the host terminal's own preference.
    Default,
    Bar,
    Block,
    Underline,
    BlockHollow,
}

/// Default colors and the live 256-entry palette (with OSC overrides).
#[derive(Debug, Clone)]
pub struct Colors {
    pub background: Rgb,
    pub foreground: Rgb,
    pub cursor: Option<Rgb>,
    pub palette: [Rgb; 256],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dirty {
    Clean,
    Partial,
    Full,
}

impl From<ffi::Rgb> for Rgb {
    fn from(c: ffi::Rgb) -> Rgb {
        Rgb {
            r: c.r,
            g: c.g,
            b: c.b,
        }
    }
}

impl From<Rgb> for ffi::Rgb {
    fn from(c: Rgb) -> ffi::Rgb {
        ffi::Rgb {
            r: c.r,
            g: c.g,
            b: c.b,
        }
    }
}

/// `styled` is the style's tagged color; `resolved` is the native
/// flattened fg/bg query, which also covers colors packed into the cell
/// (erase fills) that the style never sees. `GHOSTTY_INVALID_VALUE` is
/// the one documented "no explicit color" answer; anything else fails.
pub(crate) fn cell_color(
    styled: ffi::StyleColor,
    resolved: ffi::Rgb,
    rc: i32,
) -> Result<CellColor, Error> {
    if rc != ffi::SUCCESS && rc != ffi::INVALID_VALUE {
        return Err(Error::Ghostty(rc));
    }
    unsafe {
        match styled.tag {
            ffi::COLOR_RGB => Ok(CellColor::Rgb(styled.value.rgb.into())),
            ffi::COLOR_PALETTE => Ok(CellColor::Palette(styled.value.palette)),
            _ if rc == ffi::SUCCESS => Ok(CellColor::Rgb(resolved.into())),
            _ if rc == ffi::INVALID_VALUE => Ok(CellColor::Default),
            _ => Err(Error::Ghostty(rc)),
        }
    }
}

/// A style color with no packed fallback — underline colors only exist
/// through the style tag, so `NONE` must stay `Default` (inherited).
pub(crate) fn style_color(styled: ffi::StyleColor) -> CellColor {
    cell_color(styled, ffi::Rgb::default(), ffi::INVALID_VALUE).unwrap_or(CellColor::Default)
}

impl Colors {
    /// Resolve a cell color to RGB: `Default` picks `default` (the
    /// terminal's current default fg/bg), `Palette` looks the index up in
    /// the live palette.
    pub fn resolve(&self, color: CellColor, default: Rgb) -> Rgb {
        match color {
            CellColor::Default => default,
            CellColor::Palette(i) => self.palette[i as usize],
            CellColor::Rgb(c) => c,
        }
    }
}

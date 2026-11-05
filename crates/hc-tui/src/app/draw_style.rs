// Derived from herdr src/pane/terminal.rs (Apache-2.0).
use super::*;

/// The reference `modifier_with_underline_style` (protocol/wire.rs:1588):
/// kind 1 single / 2 double / 3 curly / 4 dotted / 5 dashed rides in the
/// high nibble so the ANSI path can emit `4:N`.
pub(super) fn modifier_with_underline_style(modifier: Modifier, underline_style: u8) -> Modifier {
    let bits = modifier.bits() | ((u16::from(underline_style) & 0x0F) << UNDERLINE_STYLE_SHIFT);
    Modifier::from_bits_retain(bits)
}

/// Ported from herdr src/pane/terminal.rs — `ghostty_cell_style`,
/// `ghostty_default_fg/bg` and `PaletteOverrides` (Apache-2.0).
///
/// A cell with no explicit color resolves to the *default* side color:
/// `None` (transparent — the host's own default shows through) when the
/// effective default still equals the host's reported answer, or the
/// VT's post-seed initial when the host was never probed; `Some(Rgb)`
/// only when the program actually changed the default (`OSC 10/11`).
/// Palette indices forward as `Indexed` so the host palette resolves
/// them, except `OSC 4` overrides which must go out as RGB.
/// Inverse/invisible resolve to concrete colors before the swap —
/// never the REVERSED/HIDDEN modifiers — matching the reference.
/// Underline color is intentionally not set: the reference thin-client
/// wire drops it (no SGR58 on the outer pane output).
pub(super) fn default_side(
    effective: Rgb,
    host: Option<Rgb>,
    initial: Option<Rgb>,
) -> Option<Color> {
    let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);
    match host {
        // The host's own default is showing — transparent; a program
        // change (OSC 10/11) diverges and must paint the real color.
        Some(h) => (h != effective).then(|| rgb(effective)),
        // No host answer: changed-from-initial paints, else transparent.
        None => initial.filter(|i| *i != effective).map(|_| rgb(effective)),
    }
}

pub(super) fn cell_style(frame: &hc_vt::Frame, cell: &hc_vt::Cell, paint: &Paint) -> Style {
    let rgb = |c: Rgb| Color::Rgb(c.r, c.g, c.b);
    let default_fg = default_side(frame.colors.foreground, paint.host_fg, paint.initial_fg);
    let default_bg = default_side(frame.colors.background, paint.host_bg, paint.initial_bg);
    // `resolved_*`: the effective defaults as RGB — only used to fill a
    // transparent side before the inverse swap (the reference's
    // `resolved_fg`/`resolved_bg`).
    let resolved_fg = Some(rgb(frame.colors.foreground));
    let resolved_bg = Some(rgb(frame.colors.background));
    let cell_color = |c: &CellColor| -> Option<Color> {
        match c {
            CellColor::Default => None,
            CellColor::Palette(i) => {
                let live = frame.colors.palette[usize::from(*i)];
                match paint.initial_palette {
                    // An OSC 4 override must paint the real color — an
                    // indexed ref would resolve against the *host's*
                    // entry, discarding the override.
                    Some(initial) if live != initial[usize::from(*i)] => Some(rgb(live)),
                    _ => Some(Color::Indexed(*i)),
                }
            }
            CellColor::Rgb(c) => Some(rgb(*c)),
        }
    };
    let mut fg = cell_color(&cell.fg).or(default_fg);
    let mut bg = cell_color(&cell.bg).or(default_bg);
    if cell.invisible {
        fg = bg.or(default_bg);
    }
    if cell.inverse {
        // Fill transparent sides with the resolved defaults so the
        // swap can't produce Reset-over-same-hue invisible text.
        if bg.is_none() {
            bg = resolved_bg;
        }
        if fg.is_none() {
            fg = resolved_fg;
        }
        std::mem::swap(&mut fg, &mut bg);
    }

    let mut mods = Modifier::empty();
    if cell.bold {
        mods |= Modifier::BOLD;
    }
    if cell.faint {
        mods |= Modifier::DIM;
    }
    if cell.italic {
        mods |= Modifier::ITALIC;
    }
    if cell.strikethrough {
        mods |= Modifier::CROSSED_OUT;
    }
    if cell.blink {
        mods |= Modifier::SLOW_BLINK;
    }
    let underline_kind = match cell.underline {
        hc_vt::Underline::None => 0,
        hc_vt::Underline::Single => 1,
        hc_vt::Underline::Double => 2,
        hc_vt::Underline::Curly => 3,
        hc_vt::Underline::Dotted => 4,
        hc_vt::Underline::Dashed => 5,
    };
    if underline_kind != 0 {
        mods |= Modifier::UNDERLINED;
    }
    let mut style = Style::default();
    if let Some(fg) = fg {
        style = style.fg(fg);
    }
    if let Some(bg) = bg {
        style = style.bg(bg);
    }
    style.add_modifier(modifier_with_underline_style(mods, underline_kind))
}

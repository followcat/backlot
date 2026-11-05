// Derived from herdr src/terminal_theme.rs and src/client/terminal_geometry.rs (Apache-2.0).
//! Host-terminal facts: the outer terminal's default colors and cell
//! pitch. Colors come from the reference's query set (`OSC 10;?`,
//! `OSC 11;?`, `OSC 4;0..255;?`, `CSI ? 996 n`) written to the host at
//! startup — replies arrive interleaved with user input, so the single
//! input reader decodes them and routes each one to `apply_reply`. A
//! pane seeds its VT with these so `CellColor::Default` resolves
//! against the *host* palette — unchanged defaults stay transparent
//! over the host's own background.
//!
//! Replies that never arrive leave their slot `None`: the pane falls
//! back to comparing against the VT's initial defaults, so an
//! unresponsive terminal loses host-transparency but not correctness.

use std::io::Write;

use hc_vt::Rgb;

/// Default colors reported by the host terminal.
#[derive(Debug, Clone)]
pub struct HostTheme {
    pub foreground: Option<Rgb>,
    pub background: Option<Rgb>,
    /// `OSC 4` answers; `None` entries keep the VT's built-in entry.
    pub palette: [Option<Rgb>; 256],
}

impl Default for HostTheme {
    fn default() -> Self {
        HostTheme {
            foreground: None,
            background: None,
            palette: [None; 256],
        }
    }
}

/// Everything the pane needs to present inner cells truthfully.
#[derive(Debug, Clone, Default)]
pub struct HostFacts {
    pub theme: HostTheme,
    /// Pixel pitch of one cell — `ws_xpixel/cols`, `ws_ypixel/rows`.
    /// `Some` only when the ioctl actually reported an extent; `None`
    /// means "unknown" and is never sent upstream as a measured fact.
    pub cell_px: Option<(u32, u32)>,
}

/// A host-terminal reply routed out of the input stream by the single
/// reader — never a keystroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostReply {
    Foreground(Rgb),
    Background(Rgb),
    Palette(u8, Rgb),
    /// `CSI ? 997 ; 1 n` — the host is dark.
    Dark,
    /// `CSI ? 997 ; 2 n` — the host is light.
    Light,
    /// XTWINOPS `CSI 6 ; height ; width t` — pixel cell pitch,
    /// observed only when a `CSI 16 t` query was actually sent.
    CellSize {
        width_px: u32,
        height_px: u32,
    },
    /// `CSI ? flags u` — kitty keyboard support reply to `CSI ? u`;
    /// bit 0 means escape disambiguation is supported.
    KeyboardFlags(u16),
    /// DECRPM `CSI ? 1016 ; Ps $ y` — SGR pixel mouse status: `true`
    /// only when the host reports 2 (recognized, reset — supported and
    /// safe to enable); 1/3 (still set after our reset) and 0/4 map to
    /// `false`, matching the reference's `sgr_pixel_mouse` mapping.
    SgrPixelMouse(bool),
    /// Primary device attributes `CSI ? … c` — the probe canary: the
    /// host really processed the query sequence (the reference's
    /// `primary_device_attributes` gate on the flags reply).
    PrimaryDeviceAttributes,
}

/// Applies one decoded reply to the facts (late replies included —
/// terminals may answer after the first paint).
pub fn apply_reply(reply: HostReply, facts: &mut HostFacts) {
    match reply {
        HostReply::Foreground(c) => facts.theme.foreground = Some(c),
        HostReply::Background(c) => facts.theme.background = Some(c),
        HostReply::Palette(i, c) => facts.theme.palette[usize::from(i)] = Some(c),
        // Appearance drives no M2 surface yet; parsed so it never leaks
        // into the key path.
        HostReply::Dark | HostReply::Light => {}
        HostReply::CellSize {
            width_px,
            height_px,
        } => facts.cell_px = Some((width_px, height_px)),
        // Capability replies feed the App's probe state, not facts.
        HostReply::KeyboardFlags(_)
        | HostReply::SgrPixelMouse(_)
        | HostReply::PrimaryDeviceAttributes => {}
    }
}

/// The capability probe — the reference's `FULL_QUERY`: DECRPM for
/// SGR pixel mouse, `CSI ? u` kitty-flags query, then `CSI c`
/// primary DA.
const CAPABILITY_QUERY: &str = "\x1b[?1016$p\x1b[?u\x1b[c";
/// `HOST_COLOR_QUERY_SEQUENCE` — default fg/bg queries.
const COLOR_QUERY: &str = "\x1b]10;?\x1b\\\x1b]11;?\x1b\\";
/// `HOST_COLOR_SCHEME_QUERY_SEQUENCE` — dark/light appearance query.
pub(crate) const SCHEME_QUERY: &str = "\x1b[?996n";

/// The outer terminal's average cell size, derived from the ioctl pixel
/// extent like the reference (`width_px/columns`, `height_px/rows`).
/// `None` when the kernel reports no pixel extent (e.g. most PTYs).
fn ioctl_cell_px() -> Option<(u32, u32)> {
    let size = crossterm::terminal::window_size().ok()?;
    if size.columns == 0 || size.rows == 0 || size.width == 0 || size.height == 0 {
        return None;
    }
    Some((
        (u32::from(size.width) / u32::from(size.columns)).max(1),
        (u32::from(size.height) / u32::from(size.rows)).max(1),
    ))
}

/// Facts without a color probe — ioctl pitch when the kernel reports
/// one, `None` (unknown, not fabricated) otherwise.
pub fn unprobed_facts() -> HostFacts {
    HostFacts {
        theme: HostTheme::default(),
        cell_px: ioctl_cell_px(),
    }
}

/// `rgb:RRRR/GGGG/BBBB` (1-4 hex digits per channel, scaled to 8-bit)
/// or `#RGB…` — the reference `parse_rgb_color`/`parse_hex_component`.
pub(crate) fn parse_rgb_color(value: &str) -> Option<Rgb> {
    let component = |s: &str| -> Option<u8> {
        if s.is_empty() || s.len() > 4 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let v = u32::from_str_radix(s, 16).ok()?;
        let max = (1u32 << (s.len() * 4)) - 1;
        Some(((v * 255 + max / 2) / max) as u8)
    };
    if let Some(rgb) = value.strip_prefix("rgb:") {
        let mut parts = rgb.split('/');
        let color = Rgb {
            r: component(parts.next()?)?,
            g: component(parts.next()?)?,
            b: component(parts.next()?)?,
        };
        return parts.next().is_none().then_some(color);
    }
    if let Some(hex) = value.strip_prefix('#') {
        // Validate before slicing: a multi-byte char could otherwise sit
        // on a slice boundary and panic (`#ab界` is 6 bytes = digits*3).
        if !hex.is_ascii() {
            return None;
        }
        let digits = hex.len() / 3;
        if !(1..=4).contains(&digits) || hex.len() != digits * 3 {
            return None;
        }
        return Some(Rgb {
            r: component(&hex[..digits])?,
            g: component(&hex[digits..digits * 2])?,
            b: component(&hex[digits * 2..])?,
        });
    }
    None
}

/// Sends the capability, theme and appearance queries. The single
/// input reader is already running by this point — its decoder routes
/// the replies to `apply_reply`, so nothing is read or discarded here.
/// Order: kitty/DA probe, fg/bg, `?997n` scheme, then `OSC 4` entries.
/// Returns whether the `CSI 16t` cell-size query was included so the
/// reader can arm exactly that reply window.
pub fn send_theme_query() -> std::io::Result<bool> {
    let cell_query = host_cell_size_query_required();
    let mut out = std::io::stdout();
    out.write_all(query_sequence(cell_query).as_bytes())?;
    out.flush().map(|()| cell_query)
}

/// `HOST_CELL_SIZE_QUERY` — asked only when the ioctl gave no pixel
/// geometry, the reference's `host_cell_size_query_required` gate (its
/// kitty-graphics precondition has no hc-tui consumer yet; pixel mouse
/// and FACTS use the reply either way).
pub(crate) fn host_cell_size_query_required() -> bool {
    ioctl_cell_px().is_none()
}

/// The query sequence, in the reference's write order: capability
/// probe first so flags/DA replies precede color answers; fg/bg then
/// `OSC 4` entries (skipped under WSL — the reference's platform
/// gate) inside the theme query; `?996n` appearance; `CSI 16t` last.
fn query_sequence(cell_query: bool) -> String {
    let mut seq = String::from(CAPABILITY_QUERY);
    seq.push_str(&color_query_sequence());
    seq.push_str(SCHEME_QUERY);
    if cell_query {
        seq.push_str("\x1b[16t");
    }
    seq
}

/// The reference theme refresh excludes appearance, avoiding a reply loop.
pub(crate) fn color_query_sequence() -> String {
    let mut seq = String::from(COLOR_QUERY);
    if !running_inside_wsl() {
        for index in 0..=u8::MAX {
            seq.push_str(&format!("\x1b]4;{index};?\x1b\\"));
        }
    }
    seq
}

/// WSL consoles do not answer `OSC 4` palette queries — the reference's
/// `should_query_host_terminal_palette` gate.
fn running_inside_wsl() -> bool {
    std::env::var_os("WSL_DISTRO_NAME").is_some()
        || std::env::var_os("WSL_INTEROP").is_some()
        || std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|s| s.to_lowercase().contains("microsoft") || s.contains("WSL"))
            .unwrap_or(false)
}

/// `Rgb` → `#rrggbb` — the wire `Display` color form.
fn rgb_hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

impl HostFacts {
    /// The attach `Display` fact — `None` until something was measured
    /// (an unprobed terminal sends nothing upstream).
    pub fn display(&self) -> Option<hc_proto::terminal::Display> {
        let display = hc_proto::terminal::Display {
            fg: self.theme.foreground.map(rgb_hex),
            bg: self.theme.background.map(rgb_hex),
            cell: self.cell_px.map(|(w, h)| [w, h]),
            cursor: None,
        };
        (display.fg.is_some() || display.bg.is_some() || display.cell.is_some()).then_some(display)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw_events::{self, Decoded};

    /// Feeds a complete OSC reply sequence through the real extractor.
    fn feed(facts: &mut HostFacts, seq: &str) {
        if let Some((Decoded::Reply(r), _)) = raw_events::extract_one_event(seq.as_bytes()) {
            apply_reply(r, facts);
        }
    }

    #[test]
    fn parses_default_and_palette_replies() {
        let mut facts = HostFacts::default();
        feed(&mut facts, "\x1b]10;rgb:ffff/ffff/ffff\x1b\\");
        feed(&mut facts, "\x1b]11;rgb:2828/3434/3434\x1b\\");
        feed(&mut facts, "\x1b]4;1;rgb:abcd/1234/56\x1b\\");
        assert_eq!(
            facts.theme.foreground,
            Some(Rgb {
                r: 255,
                g: 255,
                b: 255
            })
        );
        assert_eq!(
            facts.theme.background,
            Some(Rgb {
                r: 0x28,
                g: 0x34,
                b: 0x34
            })
        );
        let p1 = facts.theme.palette[1].expect("palette reply");
        assert_eq!((p1.r, p1.g, p1.b), (0xab, 0x12, 0x56));
        assert!(facts.theme.palette[2].is_none());
    }

    #[test]
    fn bel_style_hash_colors_parse() {
        let mut facts = HostFacts::default();
        feed(&mut facts, "\x1b]10;#ffffff\x07");
        feed(&mut facts, "\x1b]11;#282c34\x07");
        assert_eq!(
            facts.theme.foreground,
            Some(Rgb {
                r: 255,
                g: 255,
                b: 255
            })
        );
        assert_eq!(
            facts.theme.background,
            Some(Rgb {
                r: 0x28,
                g: 0x2c,
                b: 0x34
            })
        );
    }

    #[test]
    fn cell_size_reply_updates_cell_px() {
        let mut facts = HostFacts::default();
        feed(&mut facts, "\x1b[6;20;10t");
        assert_eq!(facts.cell_px, Some((10, 20)));
    }

    #[test]
    fn malformed_replies_parse_to_nothing() {
        let mut facts = HostFacts::default();
        feed(&mut facts, "\x1b]10;rgb:zz\x1b\\");
        assert!(facts.theme.foreground.is_none());
    }

    #[test]
    fn non_ascii_hex_never_panics() {
        assert!(parse_rgb_color("#ab界").is_none());
        assert!(parse_rgb_color("rgb:f界/f/f").is_none());
    }
}

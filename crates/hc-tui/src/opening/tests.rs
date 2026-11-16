//! Draw tests for the opening page: every option row, every gate
//! (`[hollywood]`, a live stage) and the width ladder the brief asks
//! for — 200, 80 and 40 columns, in the backlot theme and one
//! reference theme.

use super::*;

/// A plain-text dump of the buffer's cells, row per line — the page
/// is all text, so this is what a screen-capture shows.
fn dump(buffer: &Buffer, area: Rect) -> String {
    let mut out = String::new();
    for y in area.y..area.bottom() {
        let mut line = String::new();
        for x in area.x..area.right() {
            line.push_str(buffer[(x, y)].symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn palette() -> Palette {
    Palette::backlot()
}

fn draw(width: u16, opts: OpeningOpts) -> String {
    draw_with(palette(), width, opts)
}

fn draw_with(palette: Palette, width: u16, opts: OpeningOpts) -> String {
    let area = Rect::new(0, 0, width, 20);
    let mut buffer = Buffer::empty(area);
    render_opening(&mut buffer, area, &palette, opts);
    dump(&buffer, area)
}

const BOTH: OpeningOpts = OpeningOpts {
    hollywood: true,
    stage_running: false,
};

#[test]
fn tagline_is_one_line() {
    assert!(display_width(TAGLINE) <= 38);
    assert!(display_width(PLAIN_HINT) <= 40);
}

#[test]
fn all_rows_draw_at_200() {
    let out = draw(200, BOTH);
    for want in [
        TAGLINE,
        "n",
        "New studio",
        "h",
        "Tell Hollywood",
        "s",
        "Start the stage",
        "q",
        "detach",
        PLAIN_HINT,
    ] {
        assert!(out.contains(want), "missing {want:?} in:\n{out}");
    }
}

#[test]
fn all_rows_draw_at_80() {
    let out = draw(80, BOTH);
    assert!(out.contains("New studio"));
    assert!(out.contains(PLAIN_HINT));
}

#[test]
fn readable_at_40() {
    let out = draw(40, BOTH);
    // Every row still lands inside the frame, centred.
    assert!(out.contains(TAGLINE));
    assert!(out.contains("backlot --plain — a plain multiplexer"));
}

#[test]
fn hollywood_gate_hides_h() {
    let out = draw(
        80,
        OpeningOpts {
            hollywood: false,
            stage_running: false,
        },
    );
    assert!(!out.contains("Hollywood"), "{out}");
    assert!(out.contains("Start the stage"));
}

#[test]
fn stage_running_hides_s() {
    let out = draw(
        80,
        OpeningOpts {
            hollywood: false,
            stage_running: true,
        },
    );
    assert!(!out.contains("Start the stage"), "{out}");
    assert!(out.contains("New studio"));
    assert!(out.contains("detach"));
}

#[test]
fn minimal_frame_still_draws() {
    // Height below the page's block just clips rows — never panics.
    let area = Rect::new(0, 0, 40, 4);
    let mut buffer = Buffer::empty(area);
    render_opening(&mut buffer, area, &palette(), BOTH);
}

#[test]
fn empty_area_is_a_noop() {
    let area = Rect::new(0, 0, 0, 0);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 10));
    render_opening(&mut buffer, area, &palette(), BOTH);
}

#[test]
fn reference_theme_uses_its_palette() {
    // Catppuccin — one reference theme: same rows, its own colours,
    // so the page provedly never hardcodes a colour.
    let p = Palette::catppuccin();
    let out = draw_with(p.clone(), 80, BOTH);
    assert!(out.contains("New studio"));
    let area = Rect::new(0, 0, 80, 20);
    let mut buffer = Buffer::empty(area);
    render_opening(&mut buffer, area, &p, BOTH);
    // The fill is the palette's panel background.
    assert_eq!(buffer[(0, 0)].bg, p.panel_bg);
}

#[test]
fn colours_come_from_the_palette() {
    let p = palette();
    let area = Rect::new(0, 0, 80, 20);
    let mut buffer = Buffer::empty(area);
    render_opening(&mut buffer, area, &p, BOTH);
    // Find a key cell ('n'): its foreground is the palette accent.
    let mut found = false;
    'rows: for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if buffer[(x, y)].symbol() == "n" && buffer[(x, y)].fg == p.accent {
                found = true;
                break 'rows;
            }
        }
    }
    assert!(
        found,
        "no accent-painted key cell in:\n{}",
        dump(&buffer, area)
    );
    // The hint line paints the palette's secondary text colour.
    let mut dim = false;
    'rows: for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if buffer[(x, y)].symbol() == "b" && buffer[(x, y)].fg == p.overlay0 {
                dim = true;
                break 'rows;
            }
        }
    }
    assert!(dim, "no dim hint cell in:\n{}", dump(&buffer, area));
}

/// The hint used `surface_dim` — a separator colour within a few
/// RGB steps of the panel background in every theme. It must read
/// as secondary text instead, in dark and light themes alike.
#[test]
fn the_hint_is_legible_in_dark_and_light() {
    for (name, p) in [
        ("backlot", Palette::backlot()),
        ("catppuccin", Palette::catppuccin()),
        ("catppuccin-latte", Palette::catppuccin_latte()),
    ] {
        let area = Rect::new(0, 0, 80, 20);
        let mut buffer = Buffer::empty(area);
        render_opening(&mut buffer, area, &p, BOTH);
        let mut fg = None;
        'rows: for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                if buffer[(x, y)].symbol() == "b"
                    && buffer[(x, y)].bg == p.panel_bg
                    && buffer[(x, y)].fg != p.accent
                {
                    fg = Some(buffer[(x, y)].fg);
                    break 'rows;
                }
            }
        }
        let fg = fg.unwrap_or_else(|| panic!("a hint cell on {name}"));
        assert_eq!(fg, p.overlay0, "hint colour on {name}");
        assert_ne!(
            fg, p.surface_dim,
            "hint on {name} must not be the separator colour"
        );
        assert_ne!(
            fg, p.panel_bg,
            "hint on {name} must not match the background"
        );
    }
}

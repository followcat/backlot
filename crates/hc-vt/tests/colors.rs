//! Colors, styles and the packed-background path of `Vt::frame`.

use hc_vt::{CellColor, Rgb, Underline, Vt};

fn vt() -> Vt {
    Vt::new(80, 24, 0)
}

fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb { r, g, b }
}

#[test]
fn truecolor_and_palette_cells_carry_source_colors() {
    let mut t = vt();
    t.feed(b"\x1b[38;2;10;20;30mA\x1b[38;5;196mB\x1b[48;5;21mC");
    let f = t.frame(0).unwrap();
    let cells = &f.rows[0].cells;
    assert_eq!(cells[0].fg, CellColor::Rgb(rgb(10, 20, 30)));
    assert_eq!(cells[1].fg, CellColor::Palette(196));
    assert_eq!(cells[2].bg, CellColor::Palette(21));
}

#[test]
fn erased_cells_keep_their_packed_background() {
    let mut t = vt();
    // Set a bg color, then erase the line: the blank cells carry the bg
    // in packed cell data even though their style is empty.
    t.feed(b"\x1b[48;2;5;6;7mxxxx\x1b[K");
    let f = t.frame(0).unwrap();
    let cells = &f.rows[0].cells;
    assert_eq!(
        cells[5].bg,
        CellColor::Rgb(rgb(5, 6, 7)),
        "erased blank keeps packed bg"
    );
    assert!(cells[5].text.is_empty());
    assert_eq!(cells[0].bg, CellColor::Rgb(rgb(5, 6, 7)));
    // Palette variant: erase with an indexed bg.
    t.feed(b"\x1b[0m\x1b[2J\x1b[H\x1b[48;5;9m\x1b[K");
    let f = t.frame(0).unwrap();
    assert_eq!(f.rows[0].cells[0].bg, CellColor::Rgb(f.colors.palette[9]));
}

#[test]
fn palette_updates_resolve_through_the_live_palette() {
    let mut t = vt();
    t.feed(b"\x1b]4;1;rgb:aa/bb/cc\x07\x1b[38;5;1mX");
    let f = t.frame(0).unwrap();
    assert_eq!(f.rows[0].cells[0].fg, CellColor::Palette(1));
    assert_eq!(f.colors.palette[1], rgb(0xaa, 0xbb, 0xcc));
    assert_eq!(
        f.colors.resolve(f.rows[0].cells[0].fg, f.colors.foreground),
        rgb(0xaa, 0xbb, 0xcc)
    );

    // A later palette change re-resolves the already-indexed cell: the
    // cell keeps its index, the live palette holds the new color.
    t.feed(b"\x1b]4;1;rgb:11/22/33\x07");
    let f = t.frame(0).unwrap();
    assert_eq!(f.rows[0].cells[0].fg, CellColor::Palette(1));
    assert_eq!(
        f.colors.resolve(f.rows[0].cells[0].fg, f.colors.foreground),
        rgb(0x11, 0x22, 0x33)
    );
}

#[test]
fn osc10_11_change_the_default_colors() {
    let mut t = vt();
    let before = t.frame(0).unwrap();
    t.feed(b"\x1b]10;rgb:fe/00/01\x07\x1b]11;rgb:02/03/fe\x07x");
    let f = t.frame(0).unwrap();
    assert_eq!(f.colors.foreground, rgb(0xfe, 0x00, 0x01));
    assert_eq!(f.colors.background, rgb(0x02, 0x03, 0xfe));
    assert_ne!(f.colors.foreground, before.colors.foreground);
    // An unstyled cell resolves against the new defaults.
    assert_eq!(
        f.colors.resolve(f.rows[0].cells[0].fg, f.colors.foreground),
        rgb(0xfe, 0x00, 0x01)
    );
}

#[test]
fn either_default_color_can_change_without_setting_the_other() {
    for (bytes, foreground, background) in [
        (
            &b"\x1b]11;rgb:12/34/56\x07x"[..],
            rgb(255, 255, 255),
            rgb(0x12, 0x34, 0x56),
        ),
        (
            &b"\x1b]10;rgb:12/34/56\x07x"[..],
            rgb(0x12, 0x34, 0x56),
            rgb(0, 0, 0),
        ),
    ] {
        let mut terminal = vt();
        let _initial = terminal.frame(0).unwrap();
        terminal.feed(bytes);
        let frame = terminal.frame(0).unwrap();
        assert_eq!(frame.colors.foreground, foreground);
        assert_eq!(frame.colors.background, background);
    }
}

#[test]
fn default_and_cursor_colors_are_reported() {
    let mut t = vt();
    t.feed(b"\x1b]12;rgb:ff/00/00\x07x");
    let f = t.frame(0).unwrap();
    assert_eq!(f.colors.cursor, Some(rgb(0xff, 0, 0)));
    // Defaults are some concrete resolved color, never absent.
    let _ = (f.colors.foreground, f.colors.background);
    assert_eq!(f.rows[0].cells[1].fg, CellColor::Default);
    assert_eq!(
        f.colors.resolve(f.rows[0].cells[1].bg, f.colors.background),
        f.colors.background
    );
}

#[test]
fn attributes_and_reset() {
    let mut t = vt();
    t.feed(b"\x1b[1mb\x1b[3mi\x1b[4mu\x1b[7mr\x1b[8mc\x1b[9ms\x1b[53mo\x1b[2md");
    let f = t.frame(0).unwrap();
    let c = |i: usize| &f.rows[0].cells[i];
    assert!(c(0).bold);
    assert!(c(1).italic);
    assert_eq!(c(2).underline, Underline::Single);
    assert!(c(3).inverse);
    assert!(c(4).invisible);
    assert!(c(5).strikethrough);
    assert!(c(6).overline);
    assert!(c(7).faint);

    t.feed(b"\x1b[0mplain");
    let f = t.frame(0).unwrap();
    let p = &f.rows[0].cells[8];
    assert!(
        !p.bold
            && !p.italic
            && !p.inverse
            && !p.invisible
            && !p.strikethrough
            && p.underline == Underline::None
    );
}

#[test]
fn underline_color_is_default_unless_explicit() {
    let mut t = vt();
    // Plain underline: no underline color set.
    t.feed(b"\x1b[4:3ma");
    // Explicit curly + RGB underline color.
    t.feed(b"\x1b[58:2:200:100:50mb");
    let f = t.frame(0).unwrap();
    let (a, b) = (&f.rows[0].cells[0], &f.rows[0].cells[1]);
    assert_eq!(a.underline, Underline::Curly);
    assert_eq!(
        a.underline_color,
        CellColor::Default,
        "unset underline color must stay Default"
    );
    assert_eq!(b.underline_color, CellColor::Rgb(rgb(200, 100, 50)));
}

#[test]
fn late_host_defaults_preserve_program_overrides_and_update_reset_targets() {
    let mut t = vt();
    let mut palette = t.frame(0).unwrap().colors.palette;
    palette[1] = rgb(0x34, 0x35, 0x36);
    t.set_default_colors(
        Some(rgb(0x11, 0x11, 0x11)),
        Some(rgb(0x22, 0x22, 0x22)),
        Some(&palette),
    )
    .unwrap();
    t.feed(b"\x1b]10;#abc123\x1b\\\x1b]11;#cba321\x1b\\\x1b]4;1;#123456\x1b\\X");

    palette[1] = rgb(0xab, 0xcd, 0xef);
    let host_fg = rgb(0xfe, 0xee, 0xee);
    let host_bg = rgb(0x55, 0x56, 0x57);
    t.set_default_colors(Some(host_fg), Some(host_bg), Some(&palette))
        .unwrap();
    let overridden = t.frame(0).unwrap().colors;
    assert_eq!(overridden.foreground, rgb(0xab, 0xc1, 0x23));
    assert_eq!(overridden.background, rgb(0xcb, 0xa3, 0x21));
    assert_eq!(overridden.palette[1], rgb(0x12, 0x34, 0x56));

    t.feed(b"\x1b]110\x1b\\\x1b]111\x1b\\\x1b]104;1\x1b\\");
    let reset = t.frame(0).unwrap().colors;
    assert_eq!(reset.foreground, host_fg);
    assert_eq!(reset.background, host_bg);
    assert_eq!(reset.palette[1], palette[1]);
}

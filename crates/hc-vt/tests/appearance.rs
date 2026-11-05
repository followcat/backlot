use hc_vt::{CursorShape, Rgb, Vt};

#[test]
fn repaint_keeps_explicit_equal_palette_overrides_and_live_defaults() {
    let mut vt = Vt::new(80, 24, 0);
    let defaults = vt.frame(0).unwrap().colors.palette;
    let color = defaults[1];
    vt.feed(
        format!(
            "\x1b]4;1;rgb:{:02x}/{:02x}/{:02x}\x07\x1b[5 q",
            color.r, color.g, color.b
        )
        .as_bytes(),
    );
    let before = vt.frame(0).unwrap();
    let saved = vt.capture_appearance().unwrap();
    let after = vt.frame(0).unwrap();
    assert_eq!(before.colors.palette, after.colors.palette);
    assert_eq!(before.cursor.position, after.cursor.position);
    assert!(vt.take_replies().unwrap().is_empty());
    vt.feed(b"\x1b[!p");
    vt.restore_appearance(&saved).unwrap();
    let mut changed = defaults;
    changed[1] = Rgb { r: 1, g: 2, b: 3 };
    changed[2] = Rgb { r: 4, g: 5, b: 6 };
    vt.set_default_colors(None, None, Some(&changed)).unwrap();
    let frame = vt.frame(0).unwrap();
    assert_eq!(frame.cursor.shape, CursorShape::Bar);
    assert_eq!(
        frame.colors.palette[1], defaults[1],
        "an explicit equal override must survive later default changes"
    );
    assert_eq!(
        frame.colors.palette[2], changed[2],
        "an ordinary default must remain live"
    );
    assert!(vt.take_replies().unwrap().is_empty());
}

#[test]
fn an_explicit_repaint_appearance_wins_over_saved_state() {
    let mut vt = Vt::new(80, 24, 0);
    vt.feed(b"\x1b[5 q\x1b]4;1;#123456\x07");
    let saved = vt.capture_appearance().unwrap();
    vt.feed(b"\x1b[!p\x1b[4 q\x1b]4;1;#345678\x07");
    vt.restore_appearance(&saved).unwrap();
    let frame = vt.frame(0).unwrap();
    assert_eq!(frame.cursor.shape, CursorShape::Underline);
    assert!(!frame.cursor.blinking);
    assert_eq!(
        frame.colors.palette[1],
        Rgb {
            r: 0x34,
            g: 0x56,
            b: 0x78
        }
    );
}

#[test]
fn real_program_resets_remain_resets() {
    let mut vt = Vt::new(80, 24, 0);
    vt.feed(b"\x1b[5 q\x1b]4;1;#123456\x07\x1b[0 q\x1b]104\x07");
    let saved = vt.capture_appearance().unwrap();
    vt.feed(b"\x1b[!p");
    vt.restore_appearance(&saved).unwrap();
    let frame = vt.frame(0).unwrap();
    assert!(!frame.cursor.shape_overridden);
    assert_ne!(
        frame.colors.palette[1],
        Rgb {
            r: 0x12,
            g: 0x34,
            b: 0x56
        }
    );
}

#[test]
fn default_cursor_keeps_the_authoritative_painted_blink_mode() {
    let mut vt = Vt::new(80, 24, 0);
    let saved = vt.capture_appearance().unwrap();
    vt.feed(b"\x1b[!p\x1b[?12h");
    vt.restore_appearance(&saved).unwrap();
    assert!(vt.mode(12).unwrap());
    assert!(!vt.frame(0).unwrap().cursor.shape_overridden);
}

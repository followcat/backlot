//! Cursor policy follows pinned herdr's DECSCUSR tracker.
use hc_vt::{CursorShape, Vt};

#[test]
fn default_cursor_policy_survives_blink_modes_and_fragmented_shape_reset() {
    let mut vt = Vt::new(20, 3, 0);
    assert!(!vt.frame(0).unwrap().cursor.shape_overridden);
    vt.feed(b"\x1b[?12l");
    assert!(!vt.frame(0).unwrap().cursor.shape_overridden);
    vt.feed(b"\x1b[");
    vt.feed(b"5 ");
    vt.feed(b"q");
    let cursor = vt.frame(0).unwrap().cursor;
    assert!(cursor.shape_overridden && cursor.blinking);
    assert_eq!(cursor.shape, CursorShape::Bar);
    vt.feed(b"\x1b[6 q");
    let cursor = vt.frame(0).unwrap().cursor;
    assert!(cursor.shape_overridden && !cursor.blinking);
    assert_eq!(cursor.shape, CursorShape::Bar);
    vt.feed(b"\x1b[7 q"); // invalid explicit value does not reset policy
    assert!(vt.frame(0).unwrap().cursor.shape_overridden);
    vt.feed(b"\x1b[ q");
    assert!(!vt.frame(0).unwrap().cursor.shape_overridden);
    vt.feed(b"\x1b[3 q\x1b[0 q");
    assert!(!vt.frame(0).unwrap().cursor.shape_overridden);
}

#[test]
fn live_cursor_and_palette_survive_grid_resize() {
    let mut vt = hc_vt::Vt::new(87, 39, 256 * 1024);
    vt.feed(b"\x1b[5 q\x1b]12;#123456\x07\x1b]4;1;#345678\x07\x1b[31mx");
    let initial = vt.frame(0).unwrap();
    assert_eq!(initial.cursor.shape, hc_vt::CursorShape::Bar);
    for (cols, rows) in [(47, 23), (167, 49), (87, 39)] {
        vt.resize(cols, rows).unwrap();
        let frame = vt.frame(0).unwrap();
        assert_eq!(frame.cursor.shape, initial.cursor.shape);
        assert_eq!(frame.colors.cursor, initial.colors.cursor);
        assert_eq!(frame.colors.palette[1], initial.colors.palette[1]);
    }
}

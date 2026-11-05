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

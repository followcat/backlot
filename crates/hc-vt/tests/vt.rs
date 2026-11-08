//! Screen behavior of the native VT adapter: parsing, cursor, erase,
//! resize, alt screen, scrollback and snapshot ownership.

use hc_vt::{CursorShape, Dirty, Vt, Wide};

fn vt(cols: u16, rows: u16) -> Vt {
    Vt::new(cols, rows, 1 << 20)
}

fn row_text(row: &hc_vt::Row) -> String {
    row.cells.iter().map(|c| c.text.as_str()).collect()
}

#[test]
fn plain_text_lands_in_cells() {
    let mut t = vt(80, 24);
    t.feed(b"hello");
    let f = t.frame(0).unwrap();
    assert_eq!(f.cols, 80);
    assert_eq!(row_text(&f.rows[0])[..5], *"hello");
    assert_eq!(f.cursor.position, Some((5, 0)));
}

#[test]
fn fragmented_csi_and_utf8_parse_whole() {
    let mut t = vt(80, 24);
    // A split CSI (bold red) and a split 4-byte emoji, fed one byte at a
    // time, must land exactly as if written whole.
    for b in b"\x1b[1;31mhi \xf0\x9f\x99\x82".iter() {
        t.feed(&[*b]);
    }
    let f = t.frame(0).unwrap();
    assert_eq!(row_text(&f.rows[0])[..3], *"hi ");
    assert_eq!(f.rows[0].cells[0].fg, hc_vt::CellColor::Palette(1));
    assert!(f.rows[0].cells[0].bold);
    assert_eq!(f.rows[0].cells[3].text, "🙂");
}

#[test]
fn combining_and_wide_cells_have_full_graphemes() {
    let mut t = vt(80, 24);
    // e + combining acute (2 codepoints), CJK , and a 21-codepoint
    // combining chain — beyond the C example's 16-slot buffer.
    t.feed("e\u{0301}".as_bytes());
    t.feed("\u{4e16}".as_bytes());
    let long = format!("a{}", "\u{0301}".repeat(20));
    t.feed(long.as_bytes());
    let f = t.frame(0).unwrap();
    assert_eq!(f.rows[0].cells[0].text, "e\u{0301}");
    assert_eq!(f.rows[0].cells[1].text, "\u{4e16}");
    assert_eq!(f.rows[0].cells[1].wide, Wide::Wide);
    assert_eq!(f.rows[0].cells[2].wide, Wide::SpacerTail);
    assert_eq!(f.rows[0].cells[3].text, long);
    assert!(f.rows[0].cells[3].text.chars().count() > 16);
}

#[test]
fn cursor_moves_hides_and_changes_shape() {
    let mut t = vt(80, 24);
    t.feed(b"xx\x1b[5;10H"); // row 5, col 10
    let f = t.frame(0).unwrap();
    assert_eq!(f.cursor.position, Some((9, 4)));

    t.feed(b"\x1b[4 q"); // steady underline
    t.feed(b"\x1b[?25l"); // hide
    let f = t.frame(0).unwrap();
    assert_eq!(f.cursor.shape, CursorShape::Underline);
    assert!(!f.cursor.visible);

    t.feed(b"\x1b[6 q\x1b[?25h"); // steady bar, show
    let f = t.frame(0).unwrap();
    assert_eq!(f.cursor.shape, CursorShape::Bar);
    assert!(f.cursor.visible);
}

#[test]
fn erase_in_line_and_display_clear_cells() {
    let mut t = vt(10, 4);
    t.feed(b"abcdef\x1b[3G\x1b[K"); // erase from col 3
    let f = t.frame(0).unwrap();
    assert_eq!(row_text(&f.rows[0])[..2], *"ab");
    assert!(f.rows[0].cells[3].text.is_empty());

    t.feed(b"\x1b[2J\x1b[Hhi");
    let f = t.frame(0).unwrap();
    assert_eq!(row_text(&f.rows[0])[..2], *"hi");
    assert!(f.rows[0].cells[5].text.is_empty());
}

#[test]
fn resize_reflows_and_keeps_content() {
    let mut t = vt(80, 24);
    t.feed(b"hello");
    t.resize(40, 10).unwrap();
    assert_eq!(t.size().unwrap(), (40, 10));
    let f = t.frame(0).unwrap();
    assert_eq!(f.cols, 40);
    assert_eq!(row_text(&f.rows[0])[..5], *"hello");
    t.resize(80, 24).unwrap();
    assert_eq!(t.size().unwrap(), (80, 24));
}

#[test]
fn alt_screen_replaces_and_restores_main() {
    let mut t = vt(80, 24);
    t.feed(b"main");
    t.feed(b"\x1b[?1049hhi");
    assert!(t.alt_screen().unwrap());
    let f = t.frame(0).unwrap();
    assert_eq!(row_text(&f.rows[0])[..2], *"hi");

    t.feed(b"\x1b[?1049l");
    assert!(!t.alt_screen().unwrap());
    let f = t.frame(0).unwrap();
    assert_eq!(row_text(&f.rows[0])[..4], *"main");
}

#[test]
fn scrollback_is_bounded_and_reachable_as_negative_y() {
    let mut t = vt(80, 4);
    for i in 0..10u8 {
        t.feed(format!("line{i:02}\r\n").as_bytes());
    }
    let f = t.frame(16).unwrap();
    let history: Vec<String> = f.rows.iter().filter(|r| r.y < 0).map(row_text).collect();
    assert!(
        !history.is_empty(),
        "scrollback rows must appear above viewport"
    );
    assert!(
        history.iter().any(|l| l.starts_with("line0")),
        "oldest lines retained: {history:?}"
    );

    // Zero scrollback: nothing above the viewport.
    let mut t = Vt::new(80, 4, 0);
    for i in 0..10u8 {
        t.feed(format!("l{i}\r\n").as_bytes());
    }
    let f = t.frame(16).unwrap();
    assert!(f.rows.iter().all(|r| r.y >= 0));
}

#[test]
fn frame_data_is_owned_and_outlives_mutation() {
    let mut t = vt(80, 24);
    t.feed(b"before");
    let f = t.frame(0).unwrap();
    t.feed(b"\x1b[2J\x1b[Hafter");
    let _ = t.frame(0).unwrap();
    // The first snapshot still says "before": nothing borrowed.
    assert_eq!(row_text(&f.rows[0])[..6], *"before");
    let f2 = f.clone();
    assert_eq!(row_text(&f2.rows[0])[..6], *"before");
}

#[test]
fn dirty_state_tracks_updates_and_clean_consumes() {
    let mut t = vt(80, 24);
    t.feed(b"x");
    let f = t.frame(0).unwrap();
    assert_ne!(f.dirty, Dirty::Clean);
    t.clean().unwrap();
    let f = t.frame(0).unwrap();
    assert_eq!(f.dirty, Dirty::Clean);
    t.feed(b"y");
    let f = t.frame(0).unwrap();
    assert_ne!(f.dirty, Dirty::Clean);
}

#[test]
fn unsupported_graphics_and_osc_do_not_corrupt_text() {
    let mut t = vt(80, 24);
    t.feed(b"hi ");
    // Kitty graphics command + an OSC 52 clipboard request: neither may
    // be forwarded out or disturb the text grid.
    t.feed(b"\x1b_Ga=T,f=100,s=1,v=1;AAAA\x1b\\\x1b]52;c;SGVsbG8=\x07");
    t.feed(b"there");
    let f = t.frame(0).unwrap();
    assert_eq!(row_text(&f.rows[0])[..8], *"hi there");
    assert_eq!(
        t.take_replies().unwrap(),
        Vec::<u8>::new(),
        "no reply may leak to the pty"
    );
}

#[test]
fn invalid_utf8_is_tolerated() {
    let mut t = vt(80, 24);
    t.feed(b"\xff\xfea\xc3");
    let f = t.frame(0).unwrap();
    let _ = row_text(&f.rows[0]); // must not panic; replacement handling is the library's
}

#[test]
fn repeated_create_drop_resize_cycles_do_not_panic() {
    for i in 0..50u16 {
        let mut t = vt(80, 24);
        t.feed(b"cycle");
        t.resize(40 + i % 5, 10).unwrap();
        let f = t.frame(0).unwrap();
        assert_eq!(row_text(&f.rows[0])[..5], *"cycle");
        drop(t);
    }
}

#[test]
fn bells_are_counted_and_drained() {
    let mut vt = hc_vt::Vt::new(10, 4, 0);
    vt.feed(b"a\x07b\x07\x07");
    assert_eq!(vt.take_bell_count(), 3);
    assert_eq!(vt.take_bell_count(), 0);
}

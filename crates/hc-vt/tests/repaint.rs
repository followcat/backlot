//! A screen-only repaint must not push the old screen into scrollback.
//!
//! hc-tui reuses a cached VT across a tab switch: the core's first paint
//! for a warm attach is the reset, `ESC[H ESC[2J` and the screen — no
//! history lines. If libghostty treated the erase as a scroll, every
//! switch would append a copy of the screen to the scrollback. These
//! tests pin the behaviour the cache depends on.
//!
//! Finding: the erase itself does **not** scroll. What does is a screen
//! write whose last row ends in `\r\n` — that is one scrollback line per
//! repaint, and it is what a naive repaint (or the first draft of this
//! test) produces. The core's formatter emits rows separated by CRLF
//! with no trailing newline, so its paint leaves the scrollback at rest.

use hc_vt::Vt;

/// The prefix of herdr-core's `Terminal::paint` (crates/ghostty-vt, Apache-2.0)
/// with `history: 0`: cancel the parser, soft reset, every viewer mode
/// off, home, erase the display.
const PAINT_PREFIX: &[u8] = b"\x18\x1b[!p\x1b[?2026l\x1b[?9l\x1b[?1000l\x1b[?1001l\x1b[?1002l\x1b[?1003l\x1b[?1004l\
\x1b[?1005l\x1b[?1006l\x1b[?1015l\x1b[?1016l\x1b[?2004l\x1b[?1l\x1b[?66l\x1b[?2031l\x1b[?2048l\x1b[=0;1u\
\x1b[?1049l\x1b[?1047l\x1b[?47l\x1b[H\x1b[2J";

fn scrollback_rows(vt: &Vt) -> usize {
    vt.total_rows().unwrap() - usize::from(vt.size().unwrap().1)
}

#[test]
fn erase_display_on_the_primary_screen_keeps_scrollback() {
    let mut vt = Vt::new(40, 10, 1_000_000);
    for line in 0..100 {
        vt.feed(format!("line {line:03}\r\n").as_bytes());
    }
    let before = scrollback_rows(&vt);
    assert!(before >= 90, "the feed must reach the scrollback: {before}");
    // Paint the screen the way the core's formatter does: home, rows
    // separated by CRLF, no trailing newline.
    let screen = |vt: &mut Vt| {
        vt.feed(b"\x1b[H");
        for row in 0..10 {
            if row > 0 {
                vt.feed(b"\r\n");
            }
            vt.feed(format!("screen {row:02}").as_bytes());
        }
    };
    for _ in 0..10 {
        vt.feed(PAINT_PREFIX);
        screen(&mut vt);
    }
    assert_eq!(
        scrollback_rows(&vt),
        before,
        "ESC[2J on the primary screen must not scroll the screen away"
    );
}

/// The erase alone — isolated from any row writing — must leave the
/// scrollback at rest.
#[test]
fn a_bare_erase_display_does_not_scroll() {
    let mut vt = Vt::new(40, 10, 1_000_000);
    for line in 0..30 {
        vt.feed(format!("line {line:03}\r\n").as_bytes());
    }
    let before = scrollback_rows(&vt);
    for _ in 0..20 {
        vt.feed(b"\x1b[H\x1b[2J");
    }
    assert_eq!(scrollback_rows(&vt), before);
}

#[test]
fn alternates_screen_round_trip_keeps_its_screen() {
    let mut vt = Vt::new(40, 10, 1_000_000);
    // A full-screen program: alternate screen, its own paint.
    vt.feed(b"\x1b[?1049h\x1b[H\x1b[2Jfull-screen app\x1b[5;10Hstatus line");
    let screen = |vt: &mut Vt| -> String {
        vt.frame(0)
            .unwrap()
            .rows
            .iter()
            .flat_map(|row| row.cells.iter().map(|cell| cell.text.as_str()))
            .collect()
    };
    assert!(screen(&mut vt).contains("full-screen app"));
    let scrollback = scrollback_rows(&vt);
    // A warm repaint of the same alternates screen: leave it, reset,
    // erase, re-enter it, paint the screen.
    for _ in 0..5 {
        vt.feed(PAINT_PREFIX);
        vt.feed(b"\x1b[?1049h\x1b[H\x1b[2Jfull-screen app\x1b[5;10Hstatus line");
    }
    assert!(vt.alt_screen().unwrap());
    assert!(screen(&mut vt).contains("full-screen app"));
    assert!(screen(&mut vt).contains("status line"));
    assert_eq!(scrollback_rows(&vt), scrollback);
}

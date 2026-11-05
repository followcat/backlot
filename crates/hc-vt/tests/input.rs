//! Input encoding and reply separation: keys, paste, modes and the
//! bounded automatic-reply buffer.

use hc_vt::{Action, Error, Key, KeyInput, Mods, Vt};

fn vt() -> Vt {
    Vt::new(80, 24, 0)
}

fn key(t: &Vt, key: Key, mods: Mods) -> Vec<u8> {
    t.key(&KeyInput {
        key,
        mods,
        action: Action::Press,
    })
    .unwrap()
}

#[test]
fn replies_are_terminal_generated_not_user_input() {
    let mut t = vt();
    // DA + DSR produce automatic replies in the dedicated buffer.
    t.feed(b"\x1b[c\x1b[6n");
    let replies = t.take_replies().unwrap();
    assert!(
        replies.starts_with(b"\x1b[?"),
        "DA reply first: {replies:?}"
    );
    assert!(replies.ends_with(b"1;1R"), "DSR cursor report: {replies:?}");
    assert_eq!(t.take_replies().unwrap(), Vec::<u8>::new());

    // Key encoding is a value the caller sends — it never lands in replies.
    let sent = key(&t, Key::Char('a'), Mods::NONE);
    assert_eq!(sent, b"a");
    assert_eq!(t.take_replies().unwrap(), Vec::<u8>::new());
}

#[test]
fn reply_overflow_latches_and_recovers() {
    let mut t = vt();
    // Each DSR writes ~6 bytes; ~44k replies exceed the 256 KiB bound.
    let burst = b"\x1b[6n".repeat(2000);
    for _ in 0..24 {
        t.feed(&burst);
    }
    assert_eq!(t.take_replies().unwrap_err(), Error::ReplyOverflow);
    // After the latch clears, fresh replies flow again.
    t.feed(b"\x1b[6n");
    let replies = t.take_replies().unwrap();
    assert!(
        replies.ends_with(b"R"),
        "fresh reply after overflow: {replies:?}"
    );
}

#[test]
fn bracketed_paste_wraps_exactly_once_with_the_inner_mode() {
    let mut t = vt();
    // Mode off: raw text, newline -> CR.
    let out = t.paste(b"a\nb").unwrap();
    assert_eq!(out, b"a\rb");
    assert!(!out.windows(6).any(|w| w == b"\x1b[200~"));

    t.feed(b"\x1b[?2004h");
    let out = t.paste(b"a\nb").unwrap();
    assert!(out.starts_with(b"\x1b[200~"));
    assert!(out.ends_with(b"\x1b[201~"));
    assert_eq!(
        out.iter().filter(|w| **w == 0x1b).count(),
        2,
        "wrapped exactly once: {out:?}"
    );

    t.feed(b"\x1b[?2004l");
    assert_eq!(t.paste(b"a\nb").unwrap(), b"a\rb");
}

#[test]
fn unsafe_paste_bytes_are_stripped() {
    let t = vt();
    let out = t.paste(b"a\x1b[2J\x00b").unwrap();
    assert!(
        !out.contains(&0x1b) && !out.contains(&0),
        "escapes/control stripped: {out:?}"
    );
}

#[test]
fn application_cursor_keys_follow_decckm() {
    let mut t = vt();
    assert_eq!(key(&t, Key::ArrowUp, Mods::NONE), b"\x1b[A");
    t.feed(b"\x1b[?1h");
    assert_eq!(key(&t, Key::ArrowUp, Mods::NONE), b"\x1bOA");
    t.feed(b"\x1b[?1l");
    assert_eq!(key(&t, Key::ArrowUp, Mods::NONE), b"\x1b[A");
}

#[test]
fn control_alt_and_shift_text_legacy() {
    let t = vt();
    assert_eq!(key(&t, Key::Char('c'), Mods::CTRL), b"\x03");
    assert_eq!(key(&t, Key::Char('x'), Mods::ALT), b"\x1bx");
    assert_eq!(key(&t, Key::Char('A'), Mods::SHIFT), b"A");
    assert_eq!(key(&t, Key::Char('!'), Mods::SHIFT), b"!");
    assert_eq!(key(&t, Key::Enter, Mods::NONE), b"\r");
    assert_eq!(key(&t, Key::Backspace, Mods::NONE), b"\x7f");
    assert_eq!(key(&t, Key::F(5), Mods::NONE), b"\x1b[15~");
    assert_eq!(key(&t, Key::Char('ф'), Mods::NONE), "ф".as_bytes());
}

#[test]
fn kitty_keyboard_press_repeat_release() {
    let mut t = vt();
    // Negotiate disambiguate + events + alternates + all-keys.
    t.feed(b"\x1b[>15u");
    let press = key(&t, Key::Char('c'), Mods::CTRL);
    assert_eq!(press, b"\x1b[99;5u", "kitty ctrl+c: {press:?}");
    let release = t
        .key(&KeyInput {
            key: Key::Char('c'),
            mods: Mods::CTRL,
            action: Action::Release,
        })
        .unwrap();
    assert_eq!(release, b"\x1b[99;5:3u", "release event: {release:?}");
    let repeat = t
        .key(&KeyInput {
            key: Key::Char('c'),
            mods: Mods::CTRL,
            action: Action::Repeat,
        })
        .unwrap();
    assert_eq!(repeat, b"\x1b[99;5:2u", "repeat event: {repeat:?}");
    // Alternates: shifted A reports unshifted:shifted + shift mod.
    let shifted = key(&t, Key::Char('A'), Mods::SHIFT);
    assert_eq!(
        shifted, b"\x1b[97:65;2u",
        "alternate shifted key: {shifted:?}"
    );
}

#[test]
fn alt_arrow_uses_modifiers() {
    let t = vt();
    let out = key(&t, Key::ArrowLeft, Mods::ALT.union(Mods::SHIFT));
    assert_eq!(out, b"\x1b[1;4D", "alt+shift left: {out:?}");
}

#[test]
fn negotiated_nothing_tracks_native_keyboard_modes() {
    // The reference `keyboard_negotiated_nothing` reads the live
    // terminal flags: Kitty keyboard OR modifyOtherKeys2 both mean a
    // protocol is active. The probe must follow native state through
    // set, clear, soft and hard resets, and mid-sequence splits —
    // the same feeds that flip the flags.
    let mut t = vt();
    assert!(t.keyboard_negotiated_nothing());

    // CSI > 4 ; 2 m selects modifyOtherKeys2 natively.
    t.feed(b"\x1b[>4;2m");
    assert!(!t.keyboard_negotiated_nothing());

    // CSI > 4 ; 0 m clears it again.
    t.feed(b"\x1b[>4;0m");
    assert!(t.keyboard_negotiated_nothing());

    // Level 1 alone does not set the native flag — only the
    // numeric/level-2 form flips `modify_other_keys_2` upstream.
    t.feed(b"\x1b[>4;1m");
    assert!(t.keyboard_negotiated_nothing());

    // A sequence split across feeds must not be missed.
    t.feed(b"\x1b[>4;");
    t.feed(b"2m");
    assert!(!t.keyboard_negotiated_nothing());

    // DECSTR (CSI ! p) soft-resets the flag back to legacy.
    t.feed(b"\x1b[!p");
    assert!(t.keyboard_negotiated_nothing());

    // RIS clears it too, including mid-burst.
    t.feed(b"plain text \x1b[>4;2m more \x1bc");
    assert!(t.keyboard_negotiated_nothing());

    // Kitty flags alone end "nothing" without modifyOtherKeys.
    t.feed(b"\x1b[>1u");
    assert!(!t.keyboard_negotiated_nothing());
    t.feed(b"\x1b[<u");
    assert!(t.keyboard_negotiated_nothing());
}

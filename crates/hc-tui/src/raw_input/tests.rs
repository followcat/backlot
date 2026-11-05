// Derived in part from herdr src/raw_input.rs tests (Apache-2.0).
use super::*;
use crate::app::UiEvent;
use crate::app::input::MouseEventKind;
use crate::host::HostReply;
use crate::raw_events::{self, Decoded};

/// Push + classify in one step.
fn feed(f: &mut RawInputByteFramer, bytes: &[u8]) -> Vec<Decoded> {
    raw_events::events_from_framed_chunks(f.push(bytes))
}

fn flush(f: &mut RawInputByteFramer) -> Vec<Decoded> {
    raw_events::events_from_framed_chunks(f.flush_timeout())
}

fn is_escape(d: &Decoded) -> bool {
    matches!(
        d,
        Decoded::Ui(UiEvent::Key(k)) if k.input.key == hc_vt::Key::Escape
    )
}

#[test]
fn lone_escape_waits_for_the_idle_flush() {
    let mut f = RawInputByteFramer::for_host_input();
    assert!(feed(&mut f, b"\x1b").is_empty(), "ESC emitted on arrival");
    assert!(f.has_pending_input());
    assert_eq!(
        f.idle_flush_timeout_ms(false),
        RAW_INPUT_IDLE_FLUSH_TIMEOUT_MS
    );
    let out = flush(&mut f);
    assert_eq!(out.len(), 1);
    assert!(is_escape(&out[0]), "flush released {out:?}");
}

#[test]
fn escape_split_before_csi_stitches_into_the_sequence() {
    let mut f = RawInputByteFramer::for_host_input();
    assert!(feed(&mut f, b"\x1b").is_empty());
    let out = feed(&mut f, b"[A");
    assert_eq!(out.len(), 1, "split arrow key lost or doubled: {out:?}");
    assert!(matches!(
        &out[0],
        Decoded::Ui(UiEvent::Key(k)) if k.input.key == hc_vt::Key::ArrowUp
    ));
}

#[test]
fn csi_introducer_waits_for_split_reads() {
    let mut f = RawInputByteFramer::for_host_input();
    assert!(feed(&mut f, b"\x1b[").is_empty());
    // Without mouse capture the CSI introducer is ambiguous — the
    // keyboard window applies; over the limit it flushes as Alt+[.
    assert_eq!(
        f.idle_flush_timeout_ms(false),
        RAW_INPUT_IDLE_FLUSH_TIMEOUT_MS
    );
    // With mouse capture the introducer gets the longer window.
    assert_eq!(
        f.idle_flush_timeout_ms(true),
        MOUSE_ACTIVE_CSI_INTRODUCER_FLUSH_TIMEOUT_MS
    );
    let out = feed(&mut f, b"B");
    assert!(matches!(
        &out[0],
        Decoded::Ui(UiEvent::Key(k)) if k.input.key == hc_vt::Key::ArrowDown
    ));
}

#[test]
fn incomplete_csi_waits_the_long_window() {
    let mut f = RawInputByteFramer::for_host_input();
    assert!(feed(&mut f, b"\x1b[2").is_empty());
    assert_eq!(
        f.idle_flush_timeout_ms(false),
        INCOMPLETE_SEQUENCE_FLUSH_TIMEOUT_MS
    );
    let out = feed(&mut f, b"00~");
    assert!(out.is_empty(), "paste start without end must not emit");
    assert!(f.has_pending_input());
    // An unterminated paste stays held through the timeout flush.
    assert!(flush(&mut f).is_empty());
}

#[test]
fn bracketed_paste_split_across_reads_stays_literal() {
    let mut f = RawInputByteFramer::for_host_input();
    assert!(feed(&mut f, b"\x1b[20").is_empty());
    assert!(feed(&mut f, b"0~hello ").is_empty());
    assert!(feed(&mut f, b"world\x1b[20").is_empty());
    let out = feed(&mut f, b"1~");
    assert_eq!(out.len(), 1);
    assert!(
        matches!(&out[0], Decoded::Ui(UiEvent::Paste(p)) if p == b"hello world"),
        "{out:?}"
    );
    assert!(!f.has_pending_input());
}

#[test]
fn unterminated_paste_never_reinterprets_as_keys() {
    let mut f = RawInputByteFramer::for_host_input();
    assert!(feed(&mut f, b"\x1b[200~\x1b[A\x1b[B").is_empty());
    // The "arrows" are paste content — never key events, even on
    // flush. The tail waits for its terminator.
    assert!(flush(&mut f).is_empty());
    let out = feed(&mut f, b"\x1b[201~");
    assert!(
        matches!(&out[0], Decoded::Ui(UiEvent::Paste(p)) if p == b"\x1b[A\x1b[B"),
        "{out:?}"
    );
}

#[test]
fn split_utf8_stitches() {
    let mut f = RawInputByteFramer::for_host_input();
    assert!(feed(&mut f, "界".as_bytes()[..2].as_ref()).is_empty());
    let out = feed(&mut f, "界".as_bytes()[2..].as_ref());
    assert!(matches!(
        &out[0],
        Decoded::Ui(UiEvent::Key(k)) if k.input.key == hc_vt::Key::Char('界')
    ));
}

#[test]
fn sgr_mouse_decodes() {
    let mut f = RawInputByteFramer::for_host_input();
    let out = feed(&mut f, b"\x1b[<0;3;4M");
    assert!(matches!(
        &out[0],
        Decoded::Ui(UiEvent::Mouse(m))
            if matches!(m.kind, MouseEventKind::Down(_)) && m.col == 2 && m.row == 3
    ));
}

#[test]
fn host_color_reply_routes_during_an_armed_window() {
    let mut f = RawInputByteFramer::for_host_input();
    f.host_color_query_sent();
    let out = feed(&mut f, b"\x1b]11;rgb:2828/c4c4/3434\x1b\\");
    assert!(matches!(&out[0], Decoded::Reply(HostReply::Background(_))));
}

#[test]
fn user_key_between_replies_is_preserved() {
    let mut f = RawInputByteFramer::for_host_input();
    f.host_color_query_sent();
    // A real key interleaved in the reply burst is not a reply.
    let out = feed(&mut f, b"a\x1b]10;rgb:ffff/ffff/ffff\x1b\\b");
    assert!(matches!(
        &out[0],
        Decoded::Ui(UiEvent::Key(k)) if k.input.key == hc_vt::Key::Char('a')
    ));
    assert!(matches!(&out[1], Decoded::Reply(HostReply::Foreground(_))));
    assert!(matches!(
        &out[2],
        Decoded::Ui(UiEvent::Key(k)) if k.input.key == hc_vt::Key::Char('b')
    ));
}

#[test]
fn expired_host_window_stops_swallowing_escape() {
    let mut f = RawInputByteFramer::for_host_input();
    f.host_color_query_sent();
    f.age_host_reply_activity(HOST_REPLY_QUIET_WINDOW + Duration::from_millis(1));
    assert!(feed(&mut f, b"\x1b").is_empty());
    let out = flush(&mut f);
    assert!(is_escape(&out[0]), "expired query still held ESC: {out:?}");
}

#[test]
fn unanswered_color_reply_dropped_at_the_idle_limit() {
    let mut f = RawInputByteFramer::for_host_input();
    f.host_color_query_sent();
    assert!(feed(&mut f, b"\x1b]10;rgb:fff").is_empty());
    f.age_partial_host_color_reply(HOST_COLOR_REPLY_IDLE_LIMIT + Duration::from_millis(1));
    // The abandoned partial is dropped, not emitted as keys.
    assert!(feed(&mut f, b"x").iter().all(|d| matches!(
        d,
        Decoded::Ui(UiEvent::Key(k)) if k.input.key == hc_vt::Key::Char('x')
    )));
}

#[test]
fn oversized_control_tail_is_bounded_and_dropped() {
    let mut f = RawInputByteFramer::for_host_input();
    // A DCS that never terminates: the framer keeps a bounded tail
    // and the bytes never become keys.
    let mut junk = b"\x1bP".to_vec();
    junk.extend(std::iter::repeat_n(b'x', 300));
    assert!(feed(&mut f, &junk).is_empty());
    for _ in 0..10 {
        flush(&mut f);
    }
    // Garbage is gone; a real key still arrives intact.
    let out = feed(&mut f, b"z");
    assert!(
        out.iter().any(|d| matches!(
            d,
            Decoded::Ui(UiEvent::Key(k)) if k.input.key == hc_vt::Key::Char('z')
        )),
        "{out:?}"
    );
}

#[test]
fn sgr_pixel_mouse_decrpm_maps_reference_statuses() {
    let mut f = RawInputByteFramer::for_host_input();
    // 2 = recognized-and-reset: the only supported answer.
    let out = feed(&mut f, b"\x1b[?1016;2$y");
    assert!(matches!(
        &out[0],
        Decoded::Reply(HostReply::SgrPixelMouse(true))
    ));
    // 1/3 (still set after our reset) and 0/4 map to false.
    for status in *b"0134" {
        let mut g = RawInputByteFramer::for_host_input();
        let out = feed(
            &mut g,
            &[
                b'\x1b', b'[', b'?', b'1', b'0', b'1', b'6', b';', status, b'$', b'y',
            ],
        );
        assert!(matches!(
            &out[0],
            Decoded::Reply(HostReply::SgrPixelMouse(false))
        ));
    }
}

#[test]
fn appearance_report_routes() {
    let mut f = RawInputByteFramer::for_host_input();
    f.host_appearance_query_sent();
    let out = feed(&mut f, b"\x1b[?997;1n");
    assert!(matches!(&out[0], Decoded::Reply(HostReply::Dark)));
}

#[test]
fn focus_events_decode() {
    let mut f = RawInputByteFramer::for_host_input();
    let out = feed(&mut f, b"\x1b[I");
    assert!(matches!(&out[0], Decoded::Ui(UiEvent::Focus(true))));
    let out = feed(&mut f, b"\x1b[O");
    assert!(matches!(&out[0], Decoded::Ui(UiEvent::Focus(false))));
}

#[test]
fn kitty_and_modified_keys_decode() {
    let mut f = RawInputByteFramer::for_host_input();
    // Kitty `CSI 97;5u` = ctrl+a.
    let out = feed(&mut f, b"\x1b[97;5u");
    assert!(matches!(
        &out[0],
        Decoded::Ui(UiEvent::Key(k))
            if k.input.key == hc_vt::Key::Char('a')
                && k.input.mods == hc_vt::Mods::CTRL
    ));
    // modifyOtherKeys `CSI 27;5;13~` = ctrl+enter: the reference
    // `kitty_codepoint_to_keycode` table maps codepoint 13 to Enter.
    let out = feed(&mut f, b"\x1b[27;5;13~");
    assert!(
        matches!(
            &out[0],
            Decoded::Ui(UiEvent::Key(k))
                if k.input.key == hc_vt::Key::Enter
                    && k.input.mods == hc_vt::Mods::CTRL
        ),
        "{out:?}"
    );
}

#[test]
fn idle_flush_keeps_pending_but_holds_mouse_tail() {
    let mut f = RawInputByteFramer::for_host_input();
    f.set_host_escape_disambiguation_active(true);
    // Lone ESC under disambiguation: first flush holds it for the
    // mouse-tail window rather than emitting Escape.
    assert!(feed(&mut f, b"\x1b").is_empty());
    assert!(flush(&mut f).is_empty(), "ESC released too early");
    assert_eq!(
        f.held_input_flush_timeout_ms(),
        DISAMBIGUATED_MOUSE_TAIL_FLUSH_TIMEOUT_MS
    );
}

// Derived from herdr src/input/parse.rs, src/input/model.rs and
// src/raw_input.rs (Apache-2.0).
//! Regression vectors against the pinned reference's input parser.

use hc_tui::raw_events::parse_terminal_key_sequence as parse;
use hc_vt::{Action, Key, Mods};

#[test]
fn legacy_raw_input_keeps_source_text_metadata_on_one_event() {
    use hc_tui::raw_events::{Decoded, events_from_framed_chunks};

    for (bytes, generated) in [
        (&b"a"[..], Some("a")),
        (&b"L"[..], Some("L")),
        ("界".as_bytes(), Some("界")),
        (&b"\x03"[..], None),
        (&b"\n"[..], None),
    ] {
        let mut events = events_from_framed_chunks(vec![bytes.to_vec()]);
        assert_eq!(events.len(), 1);
        let Decoded::Ui(hc_tui::app::UiEvent::Key(key)) = events.pop().unwrap() else {
            panic!("legacy character must produce one key");
        };
        assert_eq!(key.text_commit.as_deref(), generated, "{bytes:?}");
        assert_eq!(key.vt_bytes, bytes);
    }
}

#[test]
fn legacy_enter_and_line_feed_remain_distinct() {
    let enter = parse("\r").unwrap();
    assert_eq!(enter.input.key, Key::Enter);
    assert_eq!(enter.input.mods, Mods::NONE);
    let lf = parse("\n").unwrap();
    assert_eq!(lf.input.key, Key::Char('j'));
    assert_eq!(lf.input.mods, Mods::CTRL);
}

#[test]
fn legacy_uppercase_carries_shift_for_configured_shortcuts() {
    let key = parse("L").unwrap();
    assert_eq!(key.input.key, Key::Char('L'));
    assert_eq!(key.input.mods, Mods::SHIFT);
    let alt = parse("\x1bL").unwrap();
    assert_eq!(alt.input.mods, Mods::ALT.union(Mods::SHIFT));
}

#[test]
fn kitty_associated_text_uses_decimal_unicode_scalars() {
    for (sequence, expected) in [
        ("\x1b[32;;97u", "a"),
        ("\x1b[32;;20320:22909u", "你好"),
        ("\x1b[32;;101:769u", "e\u{301}"),
    ] {
        let key = parse(sequence).unwrap();
        assert_eq!(key.text_commit.as_deref(), Some(expected), "{sequence:?}");
    }
}

#[test]
fn kitty_release_does_not_commit_associated_text() {
    let key = parse("\x1b[97;1:3;97u").unwrap();
    assert_eq!(key.input.action, Action::Release);
    assert_eq!(key.text_commit, None);
}

#[test]
fn matching_control_associated_text_is_a_key_without_generated_text() {
    for (sequence, expected) in [
        ("\x1b[13;1;13u", Key::Enter),
        ("\x1b[127;1;8u", Key::Backspace),
        ("\x1b[9;1;9u", Key::Tab),
        ("\x1b[27;1;27u", Key::Escape),
    ] {
        let key = parse(sequence).unwrap();
        assert_eq!(key.input.key, expected);
        assert_eq!(key.text_commit, None, "{sequence:?}");
    }
}

#[test]
fn malformed_kitty_reports_are_rejected() {
    for sequence in [
        "\x1b[97;0u",
        "\x1b[97;256u",
        "\x1b[97;badu",
        "\x1b[97;1:0u",
        "\x1b[97;1:4u",
        "\x1b[97;1:badu",
        "\x1b[97;1;97;98u",
        "\x1b[32;;1114112u",
        "\x1b[32;;20320:bad:u",
        "\x1b[32;;27u",
        "\x1b[13;1;8u",
        "\x1b[9;1;9:97u",
    ] {
        assert!(parse(sequence).is_none(), "accepted {sequence:?}");
    }
}

#[test]
fn non_latin_control_chord_uses_the_reported_physical_key() {
    // Cyrillic small es on the physical C key, with Ctrl alone.
    let key = parse("\x1b[1089::99;5u").unwrap();
    assert_eq!(key.input.key, Key::Char('c'));
    assert_eq!(key.input.mods, Mods::CTRL);
    // Ctrl+Alt may be AltGr text and retains the layout character.
    let altgr = parse("\x1b[1089::99;7u").unwrap();
    assert_eq!(altgr.input.key, Key::Char('с'));
    assert_eq!(altgr.input.mods, Mods::CTRL.union(Mods::ALT));
}

#[test]
fn hyper_and_meta_are_preserved_for_gui_matching_and_native_rejection() {
    for (sequence, expected) in [
        ("\x1b[99;17u", Mods::HYPER),
        ("\x1b[99;33u", Mods::META),
        ("\x1b[99;49u", Mods::HYPER.union(Mods::META)),
    ] {
        let key = parse(sequence).unwrap();
        assert_eq!(key.input.key, Key::Char('c'));
        assert_eq!(key.input.mods, expected, "{sequence:?}");
    }
}

#[test]
fn modified_special_keys_validate_modifiers_events_and_shape() {
    let key = parse("\x1b[1;6:3A").unwrap();
    assert_eq!(key.input.key, Key::ArrowUp);
    assert_eq!(key.input.mods, Mods::CTRL.union(Mods::SHIFT));
    assert_eq!(key.input.action, Action::Release);
    for sequence in [
        "\x1b[1;0A",
        "\x1b[1;256A",
        "\x1b[1;1:4A",
        "\x1b[10;60R",
        "\x1b[27;1;97;98~",
    ] {
        assert!(parse(sequence).is_none(), "accepted {sequence:?}");
    }
}

#[test]
fn modifier_reports_never_become_printable_private_use_text() {
    for codepoint in 57441..=57454 {
        for event in 1..=3 {
            assert!(parse(&format!("\x1b[{codepoint};2:{event}u")).is_none());
        }
    }
}

// Derived from herdr src/pane/input.rs (Apache-2.0).
//! Expected bytes captured from the pinned reference's raw input fixture.

use hc_vt::{Key, Mods, Vt, VtKeyEvent};

#[test]
fn reported_layout_punctuation_keeps_its_actual_base_key() {
    let mut vt = Vt::new(20, 3, 0);
    vt.feed(b"\x1b[>31u");
    for (character, mods, expected) in [
        ('+', Mods::NONE, &b"\x1b[43;;43u"[..]),
        ('+', Mods::CTRL, &b"\x1b[43;5u"[..]),
        ('?', Mods::NONE, &b"\x1b[63;;63u"[..]),
    ] {
        let bytes = vt
            .key_event(&VtKeyEvent {
                key: Key::Char(character),
                mods,
                unshifted_codepoint: Some(character as u32),
                utf8: Some(character.to_string()),
                ..VtKeyEvent::default()
            })
            .unwrap();
        assert_eq!(bytes, expected, "{character:?} {mods:?}");
    }
}

use hc_vt::{Action, Key, KeyInput, Mods, Vt};

#[test]
fn native_title_is_owned_and_tracks_osc_zero_and_two() {
    let mut vt = Vt::new(10, 2, 0);
    assert_eq!(vt.title().unwrap(), "");
    vt.feed(b"\x1b]0;fixture\x07");
    let first = vt.title().unwrap();
    vt.feed("\x1b]2;新标题\x1b\\".as_bytes());
    assert_eq!(first, "fixture");
    assert_eq!(vt.title().unwrap(), "新标题");
}

#[test]
fn named_non_text_keys_cannot_insert_private_use_text() {
    let mut vt = Vt::new(10, 2, 0);
    vt.feed(b"\x1b[>31u");
    for code in [
        57358, 57359, 57360, 57361, 57362, 57363, 57427, 57430, 57432, 57435, 57436, 57438, 57439,
        57440,
    ] {
        for action in [Action::Press, Action::Release] {
            let bytes = vt
                .key(&KeyInput {
                    key: Key::Functional(code),
                    mods: Mods::NONE,
                    action,
                })
                .unwrap();
            assert!(
                std::str::from_utf8(&bytes).is_ok_and(|text| !text
                    .chars()
                    .any(|ch| ('\u{e000}'..='\u{f8ff}').contains(&ch))),
                "code {code}"
            );
        }
    }
}

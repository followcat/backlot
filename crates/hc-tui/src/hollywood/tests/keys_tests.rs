//! The `[hollywood] key` resolution: default, direct, chord and the
//! refusal cases — collisions with `[keys]` and the panel chord, a
//! bare printable and an unparseable value.
use std::path::Path;

use crate::config::{KeyCombo, parse_key_combo};
use crate::hollywood::keys::{HollywoodKey, resolve_key};
use crate::panel::StudioKeys;

fn path() -> &'static Path {
    Path::new("config.toml")
}

#[test]
fn no_configured_key_is_alt_y_direct() {
    let (key, notice) = resolve_key(None, &StudioKeys::default(), None, path());
    assert_eq!(key, HollywoodKey::Direct(parse_key_combo("alt+y").unwrap()));
    assert!(notice.is_none());
    // An empty value behaves like an absent one.
    let (key, _) = resolve_key(Some("  "), &StudioKeys::default(), None, path());
    assert_eq!(key, HollywoodKey::Direct(parse_key_combo("alt+y").unwrap()));
}

#[test]
fn a_direct_binding_and_a_chord_resolve() {
    let (key, notice) = resolve_key(Some("ctrl+h"), &StudioKeys::default(), None, path());
    assert_eq!(
        key,
        HollywoodKey::Direct(parse_key_combo("ctrl+h").unwrap())
    );
    assert!(notice.is_none());
    let (key, notice) = resolve_key(Some("prefix+h"), &StudioKeys::default(), None, path());
    assert_eq!(key, HollywoodKey::Chord(parse_key_combo("h").unwrap()));
    assert!(notice.is_none());
}

#[test]
fn a_collision_with_the_studio_keys_falls_back() {
    let keys = StudioKeys::default();
    // `alt+left` is `previous_studio`.
    let (key, notice) = resolve_key(Some("alt+left"), &keys, None, path());
    assert_eq!(key, HollywoodKey::Direct(parse_key_combo("alt+y").unwrap()));
    let notice = notice.expect("a collision notices");
    assert!(notice.contains("previous_studio"), "{notice}");
    assert!(notice.contains("config.toml"), "{notice}");
    // The notice names the file and the [keys] entry — never the value.
    assert!(!notice.contains("alt+left"), "{notice}");
    // A `prefix+` chord tail colliding with a studio key refuses too.
    let (key, notice) = resolve_key(Some("prefix+alt+right"), &keys, None, path());
    assert_eq!(key, HollywoodKey::Direct(parse_key_combo("alt+y").unwrap()));
    assert!(notice.unwrap().contains("next_studio"));
    // `alt+up` is `previous_role`.
    let (key, notice) = resolve_key(Some("alt+up"), &keys, None, path());
    assert_eq!(key, HollywoodKey::Direct(parse_key_combo("alt+y").unwrap()));
    assert!(notice.unwrap().contains("previous_role"));
}

#[test]
fn a_collision_with_the_panel_chord_falls_back() {
    let tail: KeyCombo = parse_key_combo("y").unwrap();
    let (key, notice) = resolve_key(Some("prefix+y"), &StudioKeys::default(), Some(tail), path());
    assert_eq!(key, HollywoodKey::Direct(parse_key_combo("alt+y").unwrap()));
    let notice = notice.expect("a collision notices");
    assert!(notice.contains("panel"), "{notice}");
    // The direct binding `alt+y` never collides with the panel chord.
    let (key, _) = resolve_key(Some("alt+y"), &StudioKeys::default(), Some(tail), path());
    assert_eq!(key, HollywoodKey::Direct(tail_eq_alt_y()));
}

fn tail_eq_alt_y() -> KeyCombo {
    parse_key_combo("alt+y").unwrap()
}

#[test]
fn a_bare_printable_and_garbage_fall_back() {
    // `y` types text — a direct binding needs a modifier.
    let (key, notice) = resolve_key(Some("y"), &StudioKeys::default(), None, path());
    assert_eq!(key, HollywoodKey::Direct(parse_key_combo("alt+y").unwrap()));
    assert!(notice.is_some());
    // An unparseable value notices without echoing it.
    let (key, notice) = resolve_key(Some("nope+"), &StudioKeys::default(), None, path());
    assert_eq!(key, HollywoodKey::Direct(parse_key_combo("alt+y").unwrap()));
    let notice = notice.expect("an unparseable key notices");
    assert!(!notice.contains("nope"), "{notice}");
}

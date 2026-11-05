// Derived from herdr src/input/model.rs identity semantics (Apache-2.0).

use super::{Decision, Prefix};
use crate::raw_events::parse_terminal_key_sequence as parse;
use hc_vt::Key;

#[test]
fn consumed_layout_key_release_stays_consumed_after_control_is_released() {
    let live = crate::config::Config::default().live_keybinds();
    let mut prefix = Prefix::default();
    // The B key reports Cyrillic i on this layout. Ctrl lets the parser
    // resolve the shortcut to B; releasing Ctrl first restores the
    // layout character in the key-up report. Both have the same source
    // TerminalKey::identity() and the UI must consume both events.
    let press = parse("\x1b[1080::98;5u").unwrap();
    let release = parse("\x1b[1080::98;1:3u").unwrap();
    assert_eq!(press.input.key, Key::Char('b'));
    assert_eq!(press.layout_key, Some('и'));
    assert_eq!(release.input.key, Key::Char('и'));
    assert!(matches!(prefix.feed(&press, &live), Decision::Held));
    assert!(prefix.armed());
    assert!(matches!(prefix.feed(&release, &live), Decision::Held));
    assert!(prefix.armed());
    // Releasing a key twice must not leave a stale consumed-key lease.
    assert!(matches!(prefix.feed(&release, &live), Decision::ToTerm(_)));
}

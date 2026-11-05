//! Source policies that differ from a forwarded-key Vec plus swallowed queue.
use super::*;
use hc_vt::{Action, Key, KeyInput, Mods};

fn key(c: char) -> TerminalKey {
    TerminalKey::new(KeyInput {
        key: Key::Char(c),
        mods: Mods::NONE,
        action: Action::Press,
    })
}
type Table = InputLeaseTable<u8, u8, u64>;

#[test]
fn generated_text_without_physical_hint_is_not_a_release_lease() {
    let mut table = Table::default();
    let mut text = key('x');
    text.text_commit = Some("x".into());
    table.complete_press(
        InputLeaseKey::new(0, &text),
        &text,
        Some(&1),
        Some(&1),
        Some(7),
    );
    assert!(table.is_empty());
    text.physical_identity_hint = true;
    table.complete_press(
        InputLeaseKey::new(0, &text),
        &text,
        Some(&1),
        Some(&1),
        Some(7),
    );
    let repeated = table.normalize_press(&InputLeaseKey::new(0, &text), text);
    // Source treats VT text presses as fresh presses even with the release
    // hint: Linux CSI-u does not supply a native physical key record.
    assert_eq!(repeated.input.action, Action::Press);
    assert!(table.contains(&InputLeaseKey::new(0, &repeated)));
}

#[test]
fn csi_key_report_alone_does_not_make_a_duplicate_press_physical() {
    let mut table = Table::default();
    let mut key = key('b');
    key.input.mods = Mods::CTRL;
    key.vt_bytes = b"\x1b[98;5u".to_vec();
    table.complete_press(InputLeaseKey::new(0, &key), &key, Some(&1), Some(&2), None);
    assert!(table.contains(&InputLeaseKey::new(0, &key)));
    assert_eq!(
        table
            .normalize_press(&InputLeaseKey::new(0, &key), key)
            .input
            .action,
        Action::Press
    );
    assert!(table.is_empty());
}

#[test]
fn consumed_repeat_context_change_stays_suppressed_after_return() {
    let mut table = Table::default();
    let key = key('j');
    let id = InputLeaseKey::new(0, &key);
    table.complete_press(id, &key, Some(&1), Some(&1), None);
    assert!(matches!(
        table.plan_repeat(id, &key, Some(&1)),
        RepeatPlan::Reprocess { tracked: true, .. }
    ));
    assert!(matches!(
        table.plan_repeat(id, &key, Some(&2)),
        RepeatPlan::Ignore
    ));
    assert!(matches!(
        table.plan_repeat(id, &key, Some(&1)),
        RepeatPlan::Ignore
    ));
}

#[test]
fn sole_text_fallback_counts_consumed_keys_and_target_removal_keeps_other_source() {
    let mut table = Table::default();
    let mut text = key('?');
    text.text_commit = Some("?".into());
    let text_id = InputLeaseKey::new(0, &text);
    table.insert_forwarded(text_id, 7, text.clone());
    let other = key('b');
    let other_id = InputLeaseKey::new(0, &other);
    table.insert_consumed(other_id, ConsumedInputLease::SuppressRepeats);
    assert!(table.remove_sole_text_press(0).is_none());
    table.remove(&other_id);
    assert!(matches!(
        table.remove_sole_text_press(0),
        Some(InputLease::Forwarded(_))
    ));
    table.insert_forwarded(InputLeaseKey::new(1, &text), 8, text.clone());
    table.insert_forwarded(text_id, 7, text);
    assert_eq!(table.remove_target(&7).len(), 1);
    assert_eq!(table.remove_source(1).len(), 1);
    assert!(table.is_empty());
}

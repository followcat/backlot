//! The pure inventory reducer's sequence policy: stale/duplicate drops,
//! unknown-kind tolerance, unfiltered gaps as resync signals, `missed`
//! snapshots, strict atomic validation for malformed known events, and
//! the offline marker. No I/O anywhere.

mod common;

use common::inventory::{ev, term};
use common::minimal_terminal;
use hc_proto::events::{HeartbeatParams, MissedParams, SubscribeParams};
use hc_proto::inventory::Inventory;
use serde_json::json;

#[test]
fn replace_diffs_and_resets_the_epoch() {
    let mut inv = Inventory::unfiltered();
    inv.replace(50, vec![term("t7"), term("t8")]);
    // A new connection's epoch restarts seq; the replace still wins.
    let mut changed = minimal_terminal();
    changed["address"] = json!("t7");
    changed["title"] = json!("new");
    let effect = inv.replace(
        3,
        vec![serde_json::from_value(changed).unwrap(), term("t9")],
    );
    assert_eq!(inv.seq(), 3);
    assert_eq!(effect.removed, ["t8"]);
    assert_eq!(effect.created, ["t9"]);
    assert_eq!(effect.changed, ["t7"]);
    assert!(!inv.is_stale());
}

#[test]
fn stale_and_duplicate_events_never_apply() {
    let mut inv = Inventory::unfiltered();
    inv.replace(5, vec![term("t7")]);
    for seq in [4, 5] {
        let effect = inv.apply(&ev(seq, "title", Some("t7"), json!({"title": "late"})));
        assert!(effect.is_empty());
    }
    assert_eq!(inv.get("t7").unwrap().title, "");
}

#[test]
fn unknown_kinds_advance_the_sequence_untouched() {
    let mut inv = Inventory::unfiltered();
    inv.replace(1, vec![term("t7")]);
    let effect = inv.apply(&ev(2, "madeup", Some("t7"), json!({"x": 1})));
    assert!(effect.is_empty());
    assert_eq!(inv.seq(), 2);
    assert!(!inv.is_stale());
    // And an unknown kind may carry no terminal at all.
    assert!(inv.apply(&ev(3, "other", None, json!({}))).is_empty());
    assert_eq!(inv.seq(), 3);
}

#[test]
fn an_unfiltered_gap_reports_resync() {
    let mut inv = Inventory::unfiltered();
    inv.replace(1, vec![term("t7")]);
    let effect = inv.apply(&ev(4, "title", Some("t7"), json!({"title": "x"})));
    assert!(effect.resync);
    assert!(inv.is_stale());
    // The event's own data still applied — a snapshot restores confidence.
    assert_eq!(inv.get("t7").unwrap().title, "x");
    inv.replace(4, vec![term("t7")]);
    assert!(!inv.is_stale());
}

#[test]
fn a_filtered_gap_is_no_resync() {
    let params = SubscribeParams {
        terminal: Some("t7".into()),
        ..SubscribeParams::default()
    };
    let mut inv = Inventory::for_subscription(&params);
    inv.replace(1, vec![term("t7")]);
    let effect = inv.apply(&ev(9, "title", Some("t7"), json!({"title": "x"})));
    assert!(!effect.resync);
    assert!(!inv.is_stale());
    assert_eq!(inv.get("t7").unwrap().title, "x");
}

#[test]
fn a_heartbeat_ahead_reports_resync() {
    let mut inv = Inventory::unfiltered();
    inv.replace(3, vec![]);
    assert!(inv.heartbeat(&HeartbeatParams { seq: 3 }).is_empty());
    let effect = inv.heartbeat(&HeartbeatParams { seq: 8 });
    assert!(effect.resync);
    assert!(inv.is_stale());
}

#[test]
fn a_missed_replaces_even_at_the_same_seq_and_refetches_docs() {
    let mut inv = Inventory::unfiltered();
    inv.replace(7, vec![term("t7"), term("t8")]);
    let effect = inv.missed(&MissedParams {
        seq: 7,
        terminals: vec![term("t8")],
        machine: None,
    });
    assert!(effect.snapshot, "missed always refetches documents");
    assert_eq!(effect.removed, ["t7"]);
    assert_eq!(inv.seq(), 7);
    assert_eq!(inv.terminals().len(), 1);
}

#[test]
fn malformed_known_events_are_dropped_not_advanced() {
    let mut inv = Inventory::unfiltered();
    inv.replace(1, vec![term("t7")]);
    let before = inv.get("t7").unwrap().clone();
    let malformed = [
        // `{}` must not clear status/records/reported.
        ev(2, "status", Some("t7"), json!({})),
        ev(2, "title", Some("t7"), json!({"title": 3})),
        ev(2, "size", Some("t7"), json!({"cols": 80})),
        ev(2, "turn", Some("t7"), json!({"turn": "four"})),
        ev(2, "keep", Some("t7"), json!({})),
        ev(2, "lost", Some("t7"), json!({"reason": "x"})), // members missing
        ev(2, "created", Some("t7"), json!({"not": "a terminal"})),
        // terminal/record.address mismatch may never key the map.
        {
            let mut r = minimal_terminal();
            r["address"] = json!("t9");
            ev(2, "created", Some("t7"), r)
        },
        ev(2, "removed", None, json!({})),
        ev(2, "doc", None, json!({"version": 2})), // no name
        ev(2, "labels", Some("t7"), json!({"labels": "nope"})),
        // A valid labels map with a malformed record: no partial mutation.
        ev(
            2,
            "labels",
            Some("t7"),
            json!({"labels": {"a": "b"}, "record": {"bad": 1}}),
        ),
        ev(2, "foreground", Some("t7"), json!("oops")),
        ev(2, "title", None, json!({"title": "x"})), // missing terminal
    ];
    for (i, event) in malformed.iter().enumerate() {
        let effect = inv.apply(event);
        assert!(
            effect.malformed && effect.resync && inv.is_stale(),
            "case {i}: {event:?}"
        );
        assert_eq!(inv.seq(), 1, "case {i}: seq must not advance");
        assert_eq!(
            inv.get("t7").unwrap(),
            &before,
            "case {i}: record must be untouched"
        );
        assert!(inv.get("t9").is_none(), "case {i}: no mismatched key");
        // The stale flag clears only on a snapshot.
        inv.replace(1, vec![before.clone()]);
        assert!(!inv.is_stale());
    }
}

#[test]
fn a_filtered_subscription_still_drops_malformed() {
    let mut inv = Inventory::for_subscription(&SubscribeParams {
        terminal: Some("t7".into()),
        ..SubscribeParams::default()
    });
    inv.replace(1, vec![term("t7")]);
    let effect = inv.apply(&ev(9, "status", Some("t7"), json!({})));
    assert!(effect.malformed);
    assert_eq!(inv.seq(), 1);
}

#[test]
fn offline_marks_without_clearing_the_inventory() {
    let mut inv = Inventory::unfiltered();
    inv.replace(1, vec![term("t7")]);
    inv.set_offline(true);
    assert!(inv.is_offline());
    assert_eq!(inv.terminals().len(), 1, "the last inventory is preserved");
    inv.set_offline(false);
    assert!(!inv.is_offline());
}

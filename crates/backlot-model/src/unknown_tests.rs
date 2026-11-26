//! Forward-compatible unknown ledger event tests (`EventKind::Unknown`).

use std::collections::BTreeMap;

use crate::event::{Event, EventKind};
use crate::state::replay;
use crate::summary::summary;
use crate::testkit::*;
use crate::validate::validate;

#[test]
fn unknown_kind_parses_advances_seq_refuses_writes_and_marks_summary() {
    let raw = r#"{"v":1,"seq":7,"ts_ms":7000,"by":{"role":"producer"},"kind":"design.opened","topic":"auth"}"#;
    let parsed: Event = serde_json::from_str(raw).expect("unknown kind line should parse");
    assert_eq!(parsed.v, 1);
    assert_eq!(parsed.seq, 7);
    assert_eq!(parsed.ts_ms, 7000);
    assert_eq!(parsed.by.role, "producer");
    assert_eq!(parsed.kind.name(), "unknown");
    assert_eq!(parsed.kind.raw_name(), "design.opened");
    assert!(matches!(
        &parsed.kind,
        EventKind::Unknown { raw_kind } if raw_kind == "design.opened"
    ));
    assert!(
        serde_json::to_string(&parsed).is_err(),
        "unknown events are read-only and must not re-serialize"
    );

    let bad_known =
        r#"{"v":1,"seq":8,"ts_ms":8000,"by":{"role":"director"},"kind":"task.created"}"#;
    assert!(
        serde_json::from_str::<Event>(bad_known).is_err(),
        "known kinds with invalid payloads must still fail"
    );

    let mut events = base_events();
    let baseline = summary(
        &[("acme-shop".into(), replay(&events))],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        42,
    );
    let baseline_summary = serde_json::to_value(&baseline).unwrap();
    let s0 = &baseline_summary["studios"][0];
    assert!(s0.get("needs_upgrade").is_none());
    assert!(s0.get("unknown_kinds").is_none());

    events.push(parsed);
    let state = replay(&events);
    assert_eq!(state.last_seq, 7);
    assert_eq!(state.tasks.len(), 1, "existing tasks stay intact");

    let err = validate(
        &state,
        &new_ev(
            "producer",
            EventKind::MessageQueued {
                to: "director".into(),
                file: None,
                purpose: None,
                source_seq: None,
            },
        ),
    )
    .expect_err("ledger with unknown kinds must refuse appends");
    assert_eq!(
        err.to_string(),
        "this ledger has records from a newer backlot (design.opened); upgrade backlot"
    );

    let upgraded = serde_json::to_value(summary(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        42,
    ))
    .unwrap();
    assert_eq!(upgraded["studios"][0]["needs_upgrade"], true);
    assert_eq!(
        upgraded["studios"][0]["unknown_kinds"],
        serde_json::json!(["design.opened"])
    );
}

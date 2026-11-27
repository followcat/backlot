//! Forward-compatible unknown ledger event tests (`EventKind::Unknown`).

use std::collections::BTreeMap;

use crate::event::{Event, EventKind};
use crate::state::replay;
use crate::summary::summary;
use crate::testkit::*;
use crate::validate::validate;

#[test]
fn unknown_kind_parses_advances_seq_refuses_writes_and_marks_summary() {
    let raw = r#"{"v":1,"seq":7,"ts_ms":7000,"by":{"role":"producer"},"kind":"future.annotated","topic":"auth"}"#;
    let parsed: Event = serde_json::from_str(raw).expect("unknown kind line should parse");
    assert_eq!(parsed.v, 1);
    assert_eq!(parsed.seq, 7);
    assert_eq!(parsed.ts_ms, 7000);
    assert_eq!(parsed.by.role, "producer");
    assert_eq!(parsed.kind.name(), "unknown");
    assert_eq!(parsed.kind.raw_name(), "future.annotated");
    assert!(matches!(
        &parsed.kind,
        EventKind::Unknown { raw_kind } if raw_kind == "future.annotated"
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
        "this ledger has records from a newer backlot (future.annotated); upgrade backlot"
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
        serde_json::json!(["future.annotated"])
    );
}

#[test]
fn studio_halted_and_resumed_replay_and_unknown_compat() {
    let mut events = base_events();
    let halt_line = r#"{"v":1,"seq":4,"ts_ms":4000,"by":{"role":"producer"},"kind":"studio.halted","reason":"wrong base"}"#;
    let halt_ev: Event = serde_json::from_str(halt_line).expect("parse studio.halted");
    assert!(
        matches!(
            &halt_ev.kind,
            EventKind::StudioHalted { reason } if reason == "wrong base"
        ),
        "expected EventKind::StudioHalted, got {:?}",
        halt_ev.kind
    );
    events.push(halt_ev);

    let halted_state = replay(&events);
    let halted = halted_state
        .halted
        .as_ref()
        .expect("state.halted must be populated on replay");
    assert_eq!(halted.by, "producer");
    assert_eq!(halted.reason, "wrong base");
    assert_eq!(halted.since_ms, 4000);

    let sum_halted = serde_json::to_value(summary(
        &[("acme-shop".into(), halted_state.clone())],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        5000,
    ))
    .unwrap();
    assert_eq!(
        sum_halted["studios"][0]["halted"],
        serde_json::json!({
            "by": "producer",
            "reason": "wrong base",
            "since": 4000
        })
    );

    // Resuming clears state.halted cleanly; a future unknown event alongside still falls back cleanly.
    let resume_line = r#"{"v":1,"seq":5,"ts_ms":6000,"by":{"role":"director"},"kind":"studio.resumed","note":"fixed base","false_positive":true}"#;
    let resume_ev: Event = serde_json::from_str(resume_line).expect("parse studio.resumed");
    assert!(matches!(
        &resume_ev.kind,
        EventKind::StudioResumed { note: Some(n), false_positive: true } if n == "fixed base"
    ));
    events.push(resume_ev);
    let resumed_state = replay(&events);
    assert!(resumed_state.halted.is_none());

    let future_line = r#"{"v":1,"seq":6,"ts_ms":7000,"by":{"role":"stage"},"kind":"studio.future_stop","extra":1}"#;
    let future_ev: Event = serde_json::from_str(future_line).expect("parse future unknown");
    assert!(matches!(
        future_ev.kind,
        EventKind::Unknown { ref raw_kind } if raw_kind == "studio.future_stop"
    ));
    events.push(future_ev);
    let compat_state = replay(&events);
    assert!(compat_state.unknown_kinds.contains("studio.future_stop"));
}

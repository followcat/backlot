use serde_json::{Value, json};

use super::migrate::*;
use crate::event::{Event, EventKind, LEDGER_VERSION, NewEvent};
use crate::hollywood::HollywoodEvent;
use crate::testkit::*;

/// A line with no `v` is a v1 line — it gains the field and reports
/// `changed`, with everything else untouched.
#[test]
fn a_missing_v_is_filled_in_as_v1() {
    let val = json!({"seq": 7, "ts_ms": 3, "by": "writer", "kind": "note"});
    let (migrated, changed) = migrate_event_value(val).unwrap();
    assert!(changed);
    assert_eq!(migrated["v"], json!(LEDGER_VERSION));
    assert_eq!(migrated["seq"], json!(7));
}

/// An explicit `v: 1` line is already current — value unchanged,
/// `changed` false.
#[test]
fn a_current_line_passes_through() {
    let val = json!({"v": 1, "seq": 1, "kind": "note"});
    let (migrated, changed) = migrate_event_value(val.clone()).unwrap();
    assert!(!changed);
    assert_eq!(migrated, val);
}

/// `v: 0`, a negative, a float and a non-number are all refused as
/// invalid — none is a version a step could walk from.
#[test]
fn unusable_versions_are_refused() {
    for v in [json!(0), json!(-1), json!(1.5), json!("one"), json!(null)] {
        let val = json!({"v": v, "seq": 1});
        assert_eq!(migrate_event_value(val), Err(MigrateError::InvalidVersion));
    }
}

/// A line stamped by a newer backlot is refused with the upgrade
/// message — never read as the current schema.
#[test]
fn a_future_version_is_refused_clearly() {
    let err = migrate_event_value(json!({"v": 99, "seq": 1})).unwrap_err();
    assert_eq!(
        err,
        MigrateError::UnsupportedFuture {
            found: 99,
            supported: LEDGER_VERSION
        }
    );
    assert_eq!(
        err.to_string(),
        "ledger event version 99 is newer than supported v1; upgrade backlot"
    );
}

/// A line that parses but is not an object cannot carry a version.
#[test]
fn a_non_object_line_is_refused() {
    for val in [json!("line"), json!([1, 2]), json!(4)] {
        assert_eq!(migrate_event_value(val), Err(MigrateError::MissingObject));
    }
}

/// The step-table seam: a synthetic `1 -> 2 -> 3` walk runs each
/// step in version order, bumps `v` once per step and reports the
/// line changed. Each step sees `v` at the version it migrates
/// from, so step 2 observing `v == 2` proves step 1 already ran.
#[test]
fn a_multi_step_walk_runs_every_step_in_order() {
    fn step_one(obj: &mut serde_json::Map<String, Value>) -> Result<(), String> {
        if obj.get("v") != Some(&json!(1)) {
            return Err("step 1 expected v 1".into());
        }
        obj.insert("one".into(), json!(true));
        Ok(())
    }
    fn step_two(obj: &mut serde_json::Map<String, Value>) -> Result<(), String> {
        // `v` reaching 2 proves step 1 already ran — the pipeline
        // bumped it after that step.
        if obj.get("v") != Some(&json!(2)) {
            return Err("step 2 ran before step 1".into());
        }
        obj.insert("two".into(), json!(true));
        Ok(())
    }
    let steps: &[(u32, MigrateStep)] = &[(1, step_one as MigrateStep), (2, step_two)];
    let (migrated, changed) = migrate_with(json!({"seq": 1, "kind": "note"}), 3, steps).unwrap();
    assert!(changed);
    assert_eq!(migrated["v"], json!(3));
    assert_eq!(migrated["one"], json!(true));
    assert_eq!(migrated["two"], json!(true));
    // A line already at v3 walks no steps.
    let (same, changed) = migrate_with(migrated, 3, steps).unwrap();
    assert!(!changed);
    assert_eq!(same["v"], json!(3));
    // A v2 line picks up mid-walk — only step 2 runs.
    let (migrated, changed) = migrate_with(json!({"v": 2}), 3, steps).unwrap();
    assert!(changed);
    assert_eq!(migrated["v"], json!(3));
    assert_eq!(migrated.get("one"), None);
    assert_eq!(migrated["two"], json!(true));
}

/// A step that cannot run fails as `StepFailed` naming the `from`
/// version and its reason.
#[test]
fn a_failing_step_is_reported() {
    fn bad(_: &mut serde_json::Map<String, Value>) -> Result<(), String> {
        Err("missing field".into())
    }
    let steps: &[(u32, MigrateStep)] = &[(1, bad as MigrateStep)];
    let err = migrate_with(json!({"v": 1}), 2, steps).unwrap_err();
    assert_eq!(
        err,
        MigrateError::StepFailed {
            from: 1,
            reason: "missing field".into()
        }
    );
    assert_eq!(
        err.to_string(),
        "ledger migration v1 -> v2 failed: missing field"
    );
}

/// Hollywood's lines walk the same pipeline with their own table.
#[test]
fn hollywood_lines_migrate_the_same_way() {
    let (migrated, changed) = migrate_hollywood_value(json!({"seq": 1, "kind": "note"})).unwrap();
    assert!(changed);
    assert_eq!(migrated["v"], json!(LEDGER_VERSION));
    assert!(migrate_hollywood_value(json!({"v": 99})).is_err());
}

/// Serde reads a pre-versioning line as `v: 1` and always writes
/// `v` back out — old logs stay readable, new lines stay explicit.
#[test]
fn serde_defaults_a_missing_v_and_writes_it() {
    let ev: Event = serde_json::from_str(
        r#"{"seq":2,"ts_ms":5,"by":{"role":"writer"},"kind":"message.queued","to":"director"}"#,
    )
    .unwrap();
    assert_eq!(ev.v, LEDGER_VERSION);
    let stamped = NewEvent::new(
        crate::event::EventBy::role("writer"),
        EventKind::MessageQueued {
            to: "director".into(),
            file: None,
            purpose: None,
            source_seq: None,
        },
    )
    .stamp(1, 1);
    assert_eq!(stamped.v, LEDGER_VERSION);
    let line = serde_json::to_string(&stamped).unwrap();
    assert!(line.contains("\"v\":1"), "{line}");
    let hev: HollywoodEvent = serde_json::from_str(
        r#"{"seq":2,"ts_ms":5,"by":{"role":"user"},"kind":"message.sent","to":"producer"}"#,
    )
    .unwrap();
    assert_eq!(hev.v, LEDGER_VERSION);
}

/// The stamped `ev` helper already carries the current version.
#[test]
fn testkit_events_are_versioned() {
    assert_eq!(
        ev(
            1,
            "stage",
            EventKind::MessageQueued {
                to: "director".into(),
                file: None,
                purpose: None,
                source_seq: None,
            }
        )
        .v,
        LEDGER_VERSION
    );
}

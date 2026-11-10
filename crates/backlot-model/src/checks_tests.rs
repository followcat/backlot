//! Stage-run check declarations on `studio.created` (S9b part 2):
//! the cast's `[[checks]]` and `check_env` reach the reduced config,
//! and logs written before they existed still load. Also the
//! `task.checked` append rules: the stage's alone, once per
//! (delivery, name), on a declared check of a real delivery.

use crate::event::*;
use crate::state::{StudioState, TaskStatus, apply, replay};
use crate::testkit::*;
use crate::validate::{Refusal, validate};

/// A `studio.created` written before check declarations existed
/// still loads — both fields deserialize to their empty defaults.
#[test]
fn an_old_studio_created_without_check_fields_loads() {
    let line = r#"{"seq":1,"ts_ms":0,"by":{"role":"user"},"kind":"studio.created","title":"old","repo":"/home/user/x","mode":"standard","roles":{"producer":{"runner":"r","model":"m","effort":"e","permission_profile":"p"},"director":{"runner":"r","model":"m","effort":"e","permission_profile":"p"},"writer":{"runner":"r","model":"m","effort":"e","permission_profile":"p"}},"candidates":[],"rework_cap":2,"stall_limit_s":600}"#;
    let e: Event = serde_json::from_str(line).unwrap();
    let state = replay(&[e]);
    let cfg = state.studio.expect("studio");
    assert!(cfg.check_env.is_empty());
    assert!(cfg.checks.is_empty());
}

/// The declaration lands on the reduced config: names, commands,
/// the shared environment list and per-check timeouts survive
/// `studio.created` → `StudioConfig`.
#[test]
fn the_declared_checks_reach_the_reduced_config() {
    let mut kind = studio_kind();
    let EventKind::StudioCreated {
        check_env, checks, ..
    } = &mut kind
    else {
        unreachable!()
    };
    *check_env = vec!["CARGO_HOME".into()];
    *checks = vec![
        check_decl("unit", "cargo test --lib", 600),
        check_decl("smoke", "sh smoke.sh", 30),
    ];
    let state = replay(&[ev(1, "user", kind)]);
    let cfg = state.studio.expect("studio");
    assert_eq!(cfg.check_env, ["CARGO_HOME".to_string()]);
    assert_eq!(cfg.checks.len(), 2);
    assert_eq!(cfg.checks[0].name, "unit");
    assert_eq!(cfg.checks[0].run, "cargo test --lib");
    assert_eq!(cfg.checks[0].timeout_s, 600);
    assert_eq!(cfg.checks[1].name, "smoke");
    assert_eq!(cfg.checks[1].timeout_s, 30);
}

/// The studio for the `task.checked` tests: the `unit` and `lint`
/// checks declared, t1 cast and delivered as delivery 1.
fn delivered_state() -> StudioState {
    let mut kind = studio_kind();
    let EventKind::StudioCreated { checks, .. } = &mut kind else {
        unreachable!()
    };
    *checks = vec![
        check_decl("unit", "cargo test --lib", 600),
        check_decl("lint", "cargo clippy", 60),
    ];
    replay(&[
        ev(1, "user", kind),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "first".into(),
            },
        ),
        ev(3, "director", task_created("t1", "feature", 1)),
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(5, "actor-t1-c1", delivered("t1", 1, "a", 1)),
    ])
}

/// A `task.checked` on t1 delivery 1 from `role`.
fn checked(role: &str, name: &str, delivery: u32) -> NewEvent {
    new_ev(
        role,
        EventKind::TaskChecked {
            task: "t1".into(),
            delivery,
            name: name.into(),
            exit_code: Some(0),
            timed_out: false,
            output: format!("checks/5-{name}.log"),
            duration_ms: 42,
        },
    )
}

/// Every role but `stage` hears "the stage can" — the actor's
/// `--check` stays actor-reported on `task.delivered`, never here.
#[test]
fn only_the_stage_records_a_check() {
    let state = delivered_state();
    for role in ["user", "producer", "director", "writer", "actor-t1-c1"] {
        assert_eq!(
            validate(&state, &checked(role, "unit", 1)),
            Err(Refusal::RoleBound {
                role: role.into(),
                who: "the stage"
            }),
            "{role}"
        );
    }
    assert!(validate(&state, &checked("stage", "unit", 1)).is_ok());
}

/// One record per (delivery, check name): a rerun owes only what
/// has no record, and the ledger refuses the second write.
#[test]
fn a_check_records_once() {
    let mut state = delivered_state();
    apply(
        &mut state,
        &ev(6, "stage", checked("stage", "unit", 1).kind),
    );
    assert_eq!(
        validate(&state, &checked("stage", "unit", 1)),
        Err(Refusal::CheckDone {
            task: "t1".into(),
            delivery: 1,
            name: "unit".into(),
        })
    );
    // The other declared check is still owed.
    assert!(validate(&state, &checked("stage", "lint", 1)).is_ok());
}

/// A name the cast never declared and a delivery that does not
/// exist are both refused.
#[test]
fn checks_need_a_declared_name_and_a_real_delivery() {
    let state = delivered_state();
    assert_eq!(
        validate(&state, &checked("stage", "bogus", 1)),
        Err(Refusal::UnknownCheck {
            name: "bogus".into()
        })
    );
    assert_eq!(
        validate(&state, &checked("stage", "unit", 9)),
        Err(Refusal::NoDelivery {
            task: "t1".into(),
            delivery: 9,
        })
    );
}

/// A check already running finishes and is recorded even though the
/// task moved on — here the review asked for changes first.
#[test]
fn a_check_still_records_after_the_task_moved_on() {
    let mut state = delivered_state();
    apply(
        &mut state,
        &ev(
            6,
            "director",
            reviewed("t1", 1, Verdict::ChangesRequested, vec![]),
        ),
    );
    assert_eq!(state.tasks["t1"].status, TaskStatus::Running);
    assert!(validate(&state, &checked("stage", "unit", 1)).is_ok());
}

/// The record lands on the task; `passed` means a clean exit —
/// a signal or a timeout never passes.
#[test]
fn the_checked_record_folds_with_a_strict_pass() {
    let mut state = delivered_state();
    apply(
        &mut state,
        &ev(6, "stage", checked("stage", "unit", 1).kind),
    );
    apply(
        &mut state,
        &ev(
            7,
            "stage",
            EventKind::TaskChecked {
                task: "t1".into(),
                delivery: 1,
                name: "lint".into(),
                exit_code: None,
                timed_out: true,
                output: "checks/5-lint.log".into(),
                duration_ms: 60_000,
            },
        ),
    );
    let t = &state.tasks["t1"];
    let by_name = |n: &str| t.checked.iter().find(|c| c.name == n).unwrap();
    assert!(by_name("unit").passed());
    assert!(!by_name("lint").passed());
    assert_eq!(t.checked.len(), 2);
}

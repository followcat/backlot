//! Records across a recast: wall time follows each actor's cast
//! intervals, and `later_defects` attributes `task.finding` findings
//! to whoever made the named delivery.

use crate::event::*;
use crate::records::{StudioLog, records};
use crate::state::{TaskStatus, replay};
use crate::testkit::*;

/// Wall time follows each actor's cast intervals: a recast splits the
/// task's span between the actors instead of double-counting it.
#[test]
fn wall_time_splits_at_a_recast() {
    let mut log = base_events();
    let mut seq = log.len() as u64;
    let mut push = |role: &str, kind: EventKind| {
        seq += 1;
        log.push(ev(seq, role, kind));
    };
    push("director", cast("t1", "actor-a", "a")); // seq 4
    push("actor", delivered("t1", 1, "a", 1));
    push(
        "director",
        reviewed("t1", 1, Verdict::ChangesRequested, vec![]),
    );
    push("director", cast("t1", "actor-b", "b")); // seq 7
    push("actor", delivered("t1", 2, "b", 1));
    push("director", reviewed("t1", 2, Verdict::Accepted, vec![])); // seq 9
    let recs = records(&[StudioLog {
        studio: "s1".into(),
        events: log,
    }]);
    let a = rec(&recs, "a", "feature");
    let b = rec(&recs, "b", "feature");
    // ts_ms is seq*1000: a runs cast@4000 → recast@7000, b runs to
    // the task's last event @9000.
    assert_eq!(a.wall_time_ms, 3000);
    assert_eq!(b.wall_time_ms, 2000);
    assert!(a.wall_time_ms + b.wall_time_ms <= 9000 - 3000);
}

/// `task.finding` findings become `later_defects` on the actor who
/// made the named delivery — each side of a recast gets its own.
#[test]
fn later_defects_come_from_task_finding_on_the_delivery() {
    let mut log = base_events();
    let mut seq = log.len() as u64;
    let mut push = |role: &str, kind: EventKind| {
        seq += 1;
        log.push(ev(seq, role, kind));
    };
    // t1: actor-a delivers 1 (changes), recast to actor-b who delivers
    // 2 and is accepted.
    push("director", cast("t1", "actor-a", "a"));
    push("actor", delivered("t1", 1, "a", 1));
    push(
        "director",
        reviewed("t1", 1, Verdict::ChangesRequested, vec![]),
    );
    push("director", cast("t1", "actor-b", "b"));
    push("actor", delivered("t1", 2, "b", 1));
    push("director", reviewed("t1", 2, Verdict::Accepted, vec![]));
    // Integration finds one defect on each delivery, after closure;
    // a re-found defect keeps its id.
    push(
        "producer",
        task_finding("t1", 1, vec![finding("D8", Cause::DeliveryDefect)]),
    );
    push(
        "producer",
        task_finding(
            "t1",
            2,
            vec![
                finding("D9", Cause::RegressionFromFix),
                finding("D8", Cause::DeliveryDefect),
            ],
        ),
    );
    let state = replay(&log);
    assert_eq!(state.tasks["t1"].status, TaskStatus::Accepted);
    assert_eq!(state.tasks["t1"].finding_reports.len(), 2);
    let recs = records(&[StudioLog {
        studio: "s1".into(),
        events: log,
    }]);
    let a = rec(&recs, "a", "feature");
    let b = rec(&recs, "b", "feature");
    // D8 on delivery 1 is a's; D9 + the repeated D8 on delivery 2 are b's.
    assert_eq!(a.later_defects, 1);
    assert_eq!(b.later_defects, 2);
    // And they are not review findings: the defect counts stay put.
    assert_eq!(a.defects, 0);
    assert_eq!(b.defects, 0);
}

/// The stage's `task.checked` records count apart from the
/// `checks` an actor reports on `task.delivered` — and each lands
/// on the actor who made that delivery.
#[test]
fn stage_checks_and_actor_reported_checks_count_apart() {
    let mut kind = studio_kind();
    let EventKind::StudioCreated { checks, .. } = &mut kind else {
        unreachable!()
    };
    *checks = vec![
        check_decl("unit", "true", 60),
        check_decl("lint", "true", 60),
    ];
    let mut log = vec![ev(1, "user", kind)];
    log.extend(base_events()[1..].iter().cloned());
    let mut seq = log.len() as u64;
    let mut push = |role: &str, kind: EventKind| {
        seq += 1;
        log.push(ev(seq, role, kind));
    };
    push("director", cast("t1", "actor-a", "a"));
    // Actor-a's delivery 1 claims unit and lint pass; the stage
    // records unit pass, lint fail.
    let mut d = delivered("t1", 1, "a", 1);
    if let EventKind::TaskDelivered { checks, .. } = &mut d {
        *checks = vec![
            Check {
                name: "unit".into(),
                passed: true,
            },
            Check {
                name: "lint".into(),
                passed: true,
            },
        ];
    }
    push("actor-t1-c1", d);
    let checked = |delivery: u32, name: &str, code: Option<i32>| EventKind::TaskChecked {
        task: "t1".into(),
        delivery,
        name: name.into(),
        exit_code: code,
        timed_out: false,
        output: format!("checks/x-{name}.log"),
        duration_ms: 10,
    };
    push("stage", checked(1, "unit", Some(0)));
    push("stage", checked(1, "lint", Some(1)));
    // Recast: actor-b's delivery 2 reports nothing; the stage
    // records a pass for it on both checks.
    push("director", cast("t1", "actor-b", "b"));
    push("actor-t1-c2", delivered("t1", 2, "b", 1));
    push("stage", checked(2, "unit", Some(0)));
    push("stage", checked(2, "lint", Some(0)));
    let recs = records(&[StudioLog {
        studio: "s1".into(),
        events: log,
    }]);
    let a = rec(&recs, "a", "feature");
    let b = rec(&recs, "b", "feature");
    // a: stage recorded pass + fail on delivery 1; the actor
    // claimed both pass — the two counts stay apart.
    assert_eq!((a.stage_checks_passed, a.stage_checks_failed), (1, 1));
    assert_eq!((a.reported_checks_passed, a.reported_checks_failed), (2, 0));
    // b: only the stage's two passes — its delivery reported none.
    assert_eq!((b.stage_checks_passed, b.stage_checks_failed), (2, 0));
    assert_eq!((b.reported_checks_passed, b.reported_checks_failed), (0, 0));
}

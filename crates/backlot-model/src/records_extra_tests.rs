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

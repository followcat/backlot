//! Actor-record tests: the "Evaluation rules" bullets of ledger.md.

use std::collections::BTreeMap;

use crate::event::*;
use crate::records::{StudioLog, records};
use crate::state::{TaskStatus, replay};
use crate::testkit::*;

/// Studio "s1": t1 reworked twice then accepted; t2 recast mid-task;
/// t3 delivered under an old requirement; t4 blocked by review; t5
/// never delivered.
fn s1_log() -> Vec<Event> {
    let mut log = base_events();
    let mut seq = log.len() as u64;
    let mut push = |role: &str, kind: EventKind| {
        seq += 1;
        log.push(ev(seq, role, kind));
    };
    push("director", task_created("t2", "feature", 1));
    push("director", task_created("t3", "feature", 1));
    push("director", task_created("t4", "research", 1));
    push("director", task_created("t5", "feature", 1));
    // t1: three deliveries, defect D1 survives one rework, then accepted.
    push("director", cast("t1", "actor-a", "a"));
    push("actor", delivered("t1", 1, "a", 1));
    push(
        "director",
        reviewed(
            "t1",
            1,
            Verdict::ChangesRequested,
            vec![finding("D1", Cause::DeliveryDefect)],
        ),
    );
    push("actor", delivered("t1", 2, "a", 1));
    push(
        "director",
        reviewed(
            "t1",
            2,
            Verdict::ChangesRequested,
            vec![
                finding("D1", Cause::DeliveryDefect),
                finding("D2", Cause::RegressionFromFix),
            ],
        ),
    );
    push("actor", delivered("t1", 3, "a", 1));
    push("director", reviewed("t1", 3, Verdict::Accepted, vec![]));
    // t2: actor-a delivers once, the task is recast, actor-b finishes.
    push("director", cast("t2", "actor-a", "a"));
    push("actor", delivered("t2", 1, "a", 1));
    push(
        "director",
        reviewed(
            "t2",
            1,
            Verdict::ChangesRequested,
            vec![finding("D3", Cause::DeliveryDefect)],
        ),
    );
    push("director", cast("t2", "actor-b", "b"));
    push("actor", delivered("t2", 2, "b", 1));
    push("director", reviewed("t2", 2, Verdict::Accepted, vec![]));
    // t3: requirement bumps before the delivery, which still carries v1.
    push(
        "producer",
        EventKind::ReqVersion {
            version: 2,
            file: "req/v2.md".into(),
            summary: "scope grew".into(),
        },
    );
    push("director", cast("t3", "actor-b", "b"));
    push("actor", delivered("t3", 1, "b", 1));
    push(
        "director",
        reviewed(
            "t3",
            1,
            Verdict::Accepted,
            vec![finding("D4", Cause::RequirementChange)],
        ),
    );
    // t4: review blocks it; the finding carries the default cause.
    push("director", cast("t4", "actor-a", "a"));
    push("actor", delivered("t4", 1, "a", 1));
    push(
        "director",
        EventKind::TaskReviewed {
            task: "t4".into(),
            delivery: 1,
            reviewer: "director".into(),
            verdict: Verdict::Blocked,
            conclusion: "cannot run here".into(),
            findings: vec![Finding {
                id: "D5".into(),
                summary: "tooling missing".into(),
                severity: Severity::High,
                cause: Cause::Unattributed,
            }],
            upstream_changes: Vec::new(),
        },
    );
    log
}

/// Studio "s2": five small fix tasks for actor-a — enough data.
fn s2_log() -> Vec<Event> {
    let mut log = base_events();
    let mut seq = log.len() as u64;
    for i in 1..=5 {
        let t = format!("f{i}");
        for kind in [
            task_created(&t, "fix", 1),
            cast(&t, "actor-a", "a"),
            delivered(&t, 1, "a", 1),
            reviewed(&t, 1, Verdict::Accepted, vec![]),
        ] {
            seq += 1;
            log.push(ev(seq, "director", kind));
        }
    }
    log
}

fn s1_records() -> Vec<crate::ActorRecord> {
    records(&[StudioLog {
        studio: "s1".into(),
        events: s1_log(),
    }])
}

fn rec<'a>(recs: &'a [crate::ActorRecord], tag: &str, kind: &str) -> &'a crate::ActorRecord {
    let want = actor(tag);
    recs.iter()
        .find(|r| r.actor == want && r.task_kind == kind)
        .unwrap_or_else(|| panic!("no record for {tag}/{kind}"))
}

/// Three deliveries of one task count as one task — and a final pass
/// does not erase the earlier failures.
#[test]
fn several_deliveries_remain_one_task() {
    let recs = s1_records();
    let a = rec(&recs, "a", "feature");
    // t1 and t2 both delivered by actor-a.
    assert_eq!(a.tasks, 2);
    assert_eq!(a.deliveries, 4); // t1×3 + t2×1
    assert_eq!(a.first_delivery.changes_requested, 2);
    assert_eq!(a.first_delivery.accepted, 0);
    // Failures stay in the record even though t1 ended accepted.
    assert_eq!(a.defects, 4); // D1 twice, D2, D3
    assert_eq!(a.final_outcome.accepted, 2);
}

/// A recast attributes each delivery to who actually made it.
#[test]
fn recast_attributes_each_delivery_to_its_actor() {
    let recs = s1_records();
    let a = rec(&recs, "a", "feature");
    let b = rec(&recs, "b", "feature");
    // t2's first delivery is a's rework, its accepted second is b's first.
    assert_eq!(b.tasks, 2); // t2 and t3
    assert_eq!(b.first_delivery.accepted, 2); // t2 d2 and t3 d1
    assert_eq!(b.defects, 1); // only the requirement-change finding on t3
    // a carries t2's first-delivery failure and its defect.
    assert_eq!(a.first_delivery.changes_requested, 2);
    assert_eq!(a.actor_fault_defects, 4);
}

/// A delivery made under an older requirement is not recorded as an
/// actor failure: the finding's cause is `requirement_change`.
#[test]
fn old_requirement_delivery_is_not_actor_failure() {
    let recs = s1_records();
    let b = rec(&recs, "b", "feature");
    assert_eq!(b.defects, 1);
    assert_eq!(b.actor_fault_defects, 0);
}

/// A defect that survives a rework keeps its id and shows as a repeat.
#[test]
fn surviving_defect_keeps_its_id_as_repeat() {
    let recs = s1_records();
    let a = rec(&recs, "a", "feature");
    assert_eq!(a.defect_repeats, 1); // D1 listed again on delivery 2
    assert_eq!(a.new_in_rework, 1); // D2 first seen on delivery 2
    // Latest review of t1 lists nothing: D1 and D2 closed by rework.
    assert_eq!(a.defects_closed, 3);
    // t4's D5 sits in the latest review of that task — still open.
    let ra = rec(&recs, "a", "research");
    assert_eq!(ra.defects_open, 1);
    assert_eq!(ra.final_outcome.blocked, 1);
}

/// The default cause is `unattributed`, and it is not the actor's fault.
#[test]
fn default_cause_is_unattributed() {
    // Deserialize a finding with no `cause` field: the default applies.
    let f: Finding =
        serde_json::from_str(r#"{"id":"D9","summary":"mystery","severity":"low"}"#).unwrap();
    assert_eq!(f.cause, Cause::Unattributed);
    let recs = s1_records();
    let ra = rec(&recs, "a", "research");
    assert_eq!(ra.defects, 1);
    assert_eq!(ra.actor_fault_defects, 0);
}

/// A correction changes the view while the earlier event stays in the log.
#[test]
fn correction_changes_the_view_not_the_past() {
    let mut log = s1_log();
    let before = records(&[StudioLog {
        studio: "s1".into(),
        events: log.clone(),
    }]);
    assert_eq!(rec(&before, "b", "feature").first_delivery.accepted, 2);
    // Correct the verdict on t2's second review (accepted → changes).
    let review_seq = log
        .iter()
        .find(|e| {
            matches!(
                &e.kind,
                EventKind::TaskReviewed { task, delivery, .. }
                    if task == "t2" && *delivery == 2
            )
        })
        .unwrap()
        .seq;
    let mut set = BTreeMap::new();
    set.insert("verdict".to_string(), "changes_requested".to_string());
    let n = log.len() as u64;
    log.push(ev(
        n + 1,
        "producer",
        EventKind::Correction {
            target: review_seq,
            reason: "accepted by mistake".into(),
            set,
        },
    ));
    // The earlier event is untouched; the view changed.
    let state = replay(&log);
    assert_eq!(
        state.tasks["t2"].reviews[1].verdict,
        Verdict::ChangesRequested
    );
    assert!(matches!(
        &log[review_seq as usize - 1].kind,
        EventKind::TaskReviewed {
            verdict: Verdict::Accepted,
            ..
        }
    ));
    let after = records(&[StudioLog {
        studio: "s1".into(),
        events: log,
    }]);
    let b = rec(&after, "b", "feature");
    assert_eq!(b.first_delivery.accepted, 1); // t3's first still accepted
    assert_eq!(b.first_delivery.changes_requested, 1); // t2's corrected
    assert_eq!(b.final_outcome.open, 1); // t2 runs again
    assert_eq!(b.final_outcome.accepted, 1); // t3 accepted
}

/// Fewer than five tasks reports "not enough data".
#[test]
fn fewer_than_five_tasks_is_not_enough_data() {
    let recs = s1_records();
    assert!(!rec(&recs, "a", "feature").enough_data); // 2 tasks
    let all = records(&[
        StudioLog {
            studio: "s1".into(),
            events: s1_log(),
        },
        StudioLog {
            studio: "s2".into(),
            events: s2_log(),
        },
    ]);
    let fix = rec(&all, "a", "fix");
    assert_eq!(fix.tasks, 5);
    assert!(fix.enough_data);
    assert_eq!(fix.studios, vec!["s2".to_string()]);
    // Records stay grouped per actor identity and task kind.
    let a = rec(&all, "a", "feature");
    assert_eq!(a.tasks, 2);
    assert_eq!(a.studios, vec!["s1".to_string()]);
}

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

/// Upstream changes and takeovers land on the actor's record.
#[test]
fn upstream_changes_and_takeovers_are_counted() {
    let mut log = base_events();
    let mut seq = log.len() as u64;
    let mut push = |role: &str, kind: EventKind| {
        seq += 1;
        log.push(ev(seq, role, kind));
    };
    push("director", cast("t1", "actor-a", "a"));
    push("actor", delivered("t1", 1, "a", 1));
    push(
        "director",
        EventKind::TaskReviewed {
            task: "t1".into(),
            delivery: 1,
            reviewer: "director".into(),
            verdict: Verdict::ChangesRequested,
            conclusion: "close".into(),
            findings: vec![],
            upstream_changes: vec![
                UpstreamChange {
                    summary: "fixed the plan".into(),
                    reason: "plan was wrong".into(),
                    defect: None,
                },
                UpstreamChange {
                    summary: "rebased".into(),
                    reason: "conflict".into(),
                    defect: Some("D0".into()),
                },
            ],
        },
    );
    push(
        "director",
        EventKind::TaskState {
            task: "t1".into(),
            state: TaskStateChange::TakenOver,
            reason: "director finished it".into(),
        },
    );
    let recs = records(&[StudioLog {
        studio: "s1".into(),
        events: log,
    }]);
    let a = rec(&recs, "a", "feature");
    assert_eq!(a.upstream_changes, 2);
    assert_eq!(a.takeovers, 1);
    assert_eq!(a.final_outcome.taken_over, 1);
}

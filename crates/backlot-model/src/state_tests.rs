//! State-reduction and replay tests.

use std::collections::BTreeMap;

use crate::event::*;
use crate::state::*;
use crate::testkit::*;

/// A scripted studio run covering most event kinds.
fn script() -> Vec<Event> {
    let mut set = BTreeMap::new();
    set.insert("report".to_string(), "reports/t1-d1-fixed.md".to_string());
    vec![
        ev(1, "user", studio_kind()),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "first cut".into(),
            },
        ),
        ev(3, "director", task_created("t1", "feature", 1)),
        ev(4, "director", task_created("t2", "fix", 1)),
        ev(5, "director", cast("t1", "actor-a", "a")),
        ev(6, "actor", delivered("t1", 1, "a", 1)),
        ev(
            7,
            "director",
            reviewed(
                "t1",
                1,
                Verdict::ChangesRequested,
                vec![finding("D1", Cause::DeliveryDefect)],
            ),
        ),
        ev(8, "actor", delivered("t1", 2, "a", 1)),
        ev(9, "director", reviewed("t1", 2, Verdict::Accepted, vec![])),
        ev(
            10,
            "producer",
            EventKind::MessageQueued {
                to: "director".into(),
                file: Some("messages/10-note.md".into()),
            },
        ),
        ev(11, "stage", EventKind::MessageDelivered { target: 10 }),
        ev(
            12,
            "producer",
            EventKind::Correction {
                target: 6,
                reason: "report file was renamed".into(),
                set,
            },
        ),
        ev(
            13,
            "stage",
            EventKind::AttentionRaised {
                role: "actor".into(),
                reason: "stalled".into(),
                detail: "no output".into(),
            },
        ),
        ev(
            14,
            "stage",
            EventKind::AttentionCleared {
                target: 13,
                reason: Some("actor resumed".into()),
            },
        ),
        ev(
            15,
            "actor",
            EventKind::TaskState {
                task: "t2".into(),
                state: TaskStateChange::Abandoned,
                reason: "superseded".into(),
            },
        ),
    ]
}

/// Replay must equal applying one event at a time, at every prefix.
#[test]
fn replay_matches_one_at_a_time_at_every_prefix() {
    let log = script();
    for n in 0..=log.len() {
        let mut state = StudioState::default();
        for e in &log[..n] {
            apply(&mut state, e);
        }
        assert_eq!(state, replay(&log[..n]), "prefix {n}");
    }
}

#[test]
fn task_flow_cast_to_running_to_accepted() {
    let s = replay(&script());
    let t1 = &s.tasks["t1"];
    assert_eq!(t1.status, TaskStatus::Accepted);
    assert_eq!(t1.rework_rounds, 1);
    assert_eq!(t1.deliveries.len(), 2);
    assert_eq!(t1.cast.as_ref().unwrap().candidate, "actor-a");
    assert_eq!(s.tasks["t2"].status, TaskStatus::Abandoned);
    // Correction on seq 6 changed the delivery's report in the view.
    assert_eq!(t1.deliveries[0].report, "reports/t1-d1-fixed.md");
    assert_eq!(s.corrections.len(), 1);
    // Message link and attention lifecycle were applied.
    assert!(s.messages[0].delivered);
    assert!(!s.attention[0].open);
    assert_eq!(s.last_seq, 15);
}

/// Rework cap: the third changes_requested lands the task at `at_cap`.
#[test]
fn rework_cap_sends_task_to_at_cap() {
    let mut log = base_events();
    log.push(ev(4, "director", cast("t1", "actor-a", "a")));
    for i in 0..3u64 {
        let d = (i + 1) as u32;
        log.push(ev(10 + i * 2, "actor", delivered("t1", d, "a", 1)));
        log.push(ev(
            11 + i * 2,
            "director",
            reviewed("t1", d, Verdict::ChangesRequested, vec![]),
        ));
    }
    let s = replay(&log);
    let t = &s.tasks["t1"];
    assert_eq!(t.rework_rounds, 3);
    assert_eq!(t.status, TaskStatus::AtCap);
}

/// Pause and resume around a delivery; resume restores the old status.
#[test]
fn pause_and_resume_restore_status() {
    let mut log = base_events();
    log.push(ev(4, "director", cast("t1", "actor-a", "a")));
    log.push(ev(5, "actor", delivered("t1", 1, "a", 1)));
    log.push(ev(
        6,
        "director",
        EventKind::TaskState {
            task: "t1".into(),
            state: TaskStateChange::Paused,
            reason: "waiting on user".into(),
        },
    ));
    let s = replay(&log);
    assert_eq!(s.tasks["t1"].status, TaskStatus::Paused);
    assert_eq!(s.tasks["t1"].paused_from, Some(TaskStatus::Delivered));

    let mut log2 = log.clone();
    log2.push(ev(
        7,
        "director",
        EventKind::TaskState {
            task: "t1".into(),
            state: TaskStateChange::Resumed,
            reason: "user answered".into(),
        },
    ));
    let s2 = replay(&log2);
    assert_eq!(s2.tasks["t1"].status, TaskStatus::Delivered);
}

/// A new requirement version marks unfinished tasks as behind.
#[test]
fn new_requirement_marks_unfinished_tasks_behind() {
    let mut log = base_events();
    log.push(ev(4, "director", cast("t1", "actor-a", "a")));
    log.push(ev(
        5,
        "producer",
        EventKind::ReqVersion {
            version: 2,
            file: "req/v2.md".into(),
            summary: "grew checkout".into(),
        },
    ));
    let s = replay(&log);
    assert!(s.tasks["t1"].is_behind(s.requirement_version));
    let mut log2 = log;
    log2.push(ev(6, "actor", delivered("t1", 1, "a", 1)));
    log2.push(ev(
        7,
        "director",
        reviewed("t1", 1, Verdict::Accepted, vec![]),
    ));
    let s2 = replay(&log2);
    // Accepted under the old pin: finished, so not "behind".
    assert!(!s2.tasks["t1"].is_behind(s2.requirement_version));
}

/// A corrected verdict recomputes the task's status.
#[test]
fn corrected_verdict_recomputes_status() {
    let mut log = base_events();
    log.push(ev(4, "director", cast("t1", "actor-a", "a")));
    log.push(ev(5, "actor", delivered("t1", 1, "a", 1)));
    log.push(ev(
        6,
        "director",
        reviewed("t1", 1, Verdict::Accepted, vec![]),
    ));
    let mut set = BTreeMap::new();
    set.insert("verdict".to_string(), "changes_requested".to_string());
    log.push(ev(
        7,
        "producer",
        EventKind::Correction {
            target: 6,
            reason: "verdict clicked wrong".into(),
            set,
        },
    ));
    let s = replay(&log);
    assert_eq!(s.tasks["t1"].status, TaskStatus::Running);
    assert_eq!(s.tasks["t1"].reviews[0].verdict, Verdict::ChangesRequested);
}

/// Events serialize as one JSON object per line with the `kind` tag.
#[test]
fn event_json_shape_is_flat() {
    let e = ev(3, "director", cast("t1", "actor-a", "a"));
    let line = serde_json::to_string(&e).unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["kind"], "task.cast");
    assert_eq!(v["seq"], 3);
    assert_eq!(v["task"], "t1");
    assert_eq!(v["by"]["role"], "director");
    let back: Event = serde_json::from_str(&line).unwrap();
    assert_eq!(back, e);
}

//! `next_steps` derives every owed step from the reduced state — one
//! per obligation row, keyed on the seq of the event that created it
//! — and nothing once the studio is finished.

use crate::event::*;
use crate::state::{StudioState, apply, replay};
use crate::steps::next_steps;
use crate::testkit::*;

/// Studio + requirement v1 — the base every row needs.
fn base() -> Vec<Event> {
    vec![
        ev(1, "user", studio_kind()),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "first".into(),
            },
        ),
    ]
}

fn plan_request() -> EventKind {
    EventKind::MessageQueued {
        to: "writer".into(),
        file: None,
        purpose: Some("plan".into()),
        source_seq: None,
    }
}

fn dispatch() -> EventKind {
    EventKind::MessageQueued {
        to: "director".into(),
        file: Some("messages/6-note.md".into()),
        purpose: Some("dispatch".into()),
        source_seq: None,
    }
}

fn publish(kind: PlanKind, seq: u64) -> EventKind {
    EventKind::PlanPublished {
        plan_kind: kind,
        file: format!("plan/{seq}-doc.md"),
        requirement_version: 1,
    }
}

fn report(to: &str, node: &str) -> EventKind {
    EventKind::ReportSent {
        to: to.into(),
        node: node.into(),
        file: "reports/r.md".into(),
    }
}

/// `(key, owner)` for the step labelled `label`, if one is owed.
fn step(state: &StudioState, label: &str) -> Option<(u64, String)> {
    next_steps(state)
        .into_iter()
        .find(|s| s.label == label)
        .map(|s| (s.key, s.owner))
}

/// The writer owes until *both* documents exist after the latest
/// request — one alone changes nothing — and then the producer owes
/// the dispatch, keyed on the publish that completed the pair.
#[test]
fn a_plan_request_is_the_writers_until_published() {
    let mut events = base();
    events.push(ev(3, "producer", plan_request()));
    let mut state = replay(&events);
    assert_eq!(
        step(&state, "plan and acceptance list"),
        Some((3, "writer".into()))
    );
    apply(&mut state, &ev(4, "writer", publish(PlanKind::Plan, 4)));
    assert_eq!(
        step(&state, "plan and acceptance list"),
        Some((3, "writer".into()))
    );
    apply(
        &mut state,
        &ev(5, "writer", publish(PlanKind::Acceptance, 5)),
    );
    assert!(step(&state, "plan and acceptance list").is_none());
    assert_eq!(step(&state, "dispatch"), Some((5, "producer".into())));
    // The producer's step owes from the later publish, not the
    // request — `since_ms` is when it became owed.
    assert_eq!(
        next_steps(&state)
            .iter()
            .find(|s| s.label == "dispatch")
            .unwrap()
            .since_ms,
        5000
    );
}

/// A second request re-keys the writer's step — only publishes
/// after the *latest* one satisfy it.
#[test]
fn a_later_plan_request_restarts_the_writer() {
    let mut events = base();
    events.extend([
        ev(3, "producer", plan_request()),
        ev(4, "writer", publish(PlanKind::Plan, 4)),
        ev(5, "writer", publish(PlanKind::Acceptance, 5)),
        ev(6, "producer", plan_request()),
    ]);
    let state = replay(&events);
    assert_eq!(
        step(&state, "plan and acceptance list"),
        Some((6, "writer".into()))
    );
    assert!(step(&state, "dispatch").is_none());
}

/// The delivered dispatch is the director's until a task follows
/// it — undelivered it obliges nobody yet, and the step owes from
/// the delivery.
#[test]
fn a_delivered_dispatch_is_the_directors() {
    let mut events = base();
    events.extend([
        ev(3, "producer", dispatch()),
        ev(4, "stage", EventKind::MessageDelivered { target: 3 }),
    ]);
    let mut state = replay(&events);
    let (key, owner) = step(&state, "tasks").expect("tasks owed");
    assert_eq!((key, owner.as_str()), (3, "director"));
    assert_eq!(
        next_steps(&state)
            .iter()
            .find(|s| s.label == "tasks")
            .unwrap()
            .since_ms,
        4000
    );
    apply(
        &mut state,
        &ev(5, "director", task_created("t1", "feature", 1)),
    );
    assert!(step(&state, "tasks").is_none());
    assert_eq!(step(&state, "cast t1"), Some((5, "director".into())));

    // An undelivered dispatch owes the director nothing.
    let state = replay(
        &base()
            .into_iter()
            .chain([ev(3, "producer", dispatch())])
            .collect::<Vec<_>>(),
    );
    assert!(step(&state, "tasks").is_none());
}

/// A created-and-never-cast task is owed by whoever created it —
/// the director on the split path, the producer when it made the
/// task itself.
#[test]
fn an_uncast_task_is_its_creators() {
    let mut events = base();
    events.push(ev(3, "director", task_created("t1", "feature", 1)));
    let mut state = replay(&events);
    assert_eq!(step(&state, "cast t1"), Some((3, "director".into())));
    apply(&mut state, &ev(4, "producer", task_created("t2", "fix", 1)));
    assert_eq!(step(&state, "cast t2"), Some((4, "producer".into())));
    apply(&mut state, &ev(5, "director", cast("t1", "actor-a", "a")));
    assert!(step(&state, "cast t1").is_none());
    assert_eq!(step(&state, "deliver t1"), Some((5, "actor-t1-c1".into())));
}

/// A running task is its current actor's — a changes-requested
/// review re-keys the step on the review itself.
#[test]
fn a_running_task_is_its_actors() {
    let mut events = base();
    events.extend([
        ev(3, "director", task_created("t1", "feature", 1)),
        ev(4, "director", cast("t1", "actor-a", "a")),
    ]);
    let mut state = replay(&events);
    assert_eq!(step(&state, "deliver t1"), Some((4, "actor-t1-c1".into())));
    apply(
        &mut state,
        &ev(5, "actor-t1-c1", delivered("t1", 1, "a", 1)),
    );
    assert!(step(&state, "deliver t1").is_none());
    apply(
        &mut state,
        &ev(
            6,
            "director",
            reviewed("t1", 1, Verdict::ChangesRequested, vec![]),
        ),
    );
    assert_eq!(step(&state, "deliver t1"), Some((6, "actor-t1-c1".into())));
}

/// The latest delivery without its review is the reviewer's — the
/// director's on a director cast, the producer's on the light path.
#[test]
fn a_delivered_task_is_its_reviewers() {
    let mut events = base();
    events.extend([
        ev(3, "director", task_created("t1", "feature", 1)),
        ev(4, "producer", task_created("t2", "fix", 1)),
        ev(5, "director", cast("t1", "actor-a", "a")),
        ev(6, "producer", cast("t2", "actor-b", "b")),
        ev(7, "actor-t1-c1", delivered("t1", 1, "a", 1)),
        ev(8, "actor-t2-c1", delivered("t2", 1, "b", 1)),
    ]);
    let mut state = replay(&events);
    assert_eq!(step(&state, "review t1"), Some((7, "director".into())));
    assert_eq!(step(&state, "review t2"), Some((8, "producer".into())));
    apply(
        &mut state,
        &ev(9, "director", reviewed("t1", 1, Verdict::Accepted, vec![])),
    );
    assert!(step(&state, "review t1").is_none());
}

/// The review that takes a task past the cap hands the call to the
/// producer — keyed on that review.
#[test]
fn at_cap_is_the_producers() {
    let mut events = base();
    events.extend([
        ev(3, "director", task_created("t1", "feature", 1)),
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(5, "actor-t1-c1", delivered("t1", 1, "a", 1)),
        ev(
            6,
            "director",
            reviewed("t1", 1, Verdict::ChangesRequested, vec![]),
        ),
        ev(7, "actor-t1-c1", delivered("t1", 2, "a", 1)),
        ev(
            8,
            "director",
            reviewed("t1", 2, Verdict::ChangesRequested, vec![]),
        ),
        ev(9, "actor-t1-c1", delivered("t1", 3, "a", 1)),
    ]);
    let mut state = replay(&events);
    assert_eq!(
        state.tasks["t1"].status,
        crate::state::TaskStatus::Delivered
    );
    apply(
        &mut state,
        &ev(
            10,
            "director",
            reviewed("t1", 3, Verdict::ChangesRequested, vec![]),
        ),
    );
    assert_eq!(state.tasks["t1"].status, crate::state::TaskStatus::AtCap);
    assert_eq!(step(&state, "t1 at cap"), Some((10, "producer".into())));
}

/// A blocked task is its reviewer's to unblock — whether the actor
/// blocked it or a blocked verdict did.
#[test]
fn a_blocked_task_is_its_reviewers() {
    let mut events = base();
    events.extend([
        ev(3, "director", task_created("t1", "feature", 1)),
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(
            5,
            "actor-t1-c1",
            task_state("t1", TaskStateChange::Blocked, "waiting on api"),
        ),
    ]);
    let mut state = replay(&events);
    assert_eq!(step(&state, "unblock t1"), Some((5, "director".into())));
    apply(
        &mut state,
        &ev(
            6,
            "director",
            task_state("t1", TaskStateChange::Resumed, "answered"),
        ),
    );
    assert!(step(&state, "unblock t1").is_none());
    apply(
        &mut state,
        &ev(7, "actor-t1-c1", delivered("t1", 1, "a", 1)),
    );
    apply(
        &mut state,
        &ev(8, "director", reviewed("t1", 1, Verdict::Blocked, vec![])),
    );
    assert_eq!(step(&state, "unblock t1"), Some((8, "director".into())));
}

/// Every task terminal with no node report after the last close:
/// the director owes it — the producer when it cast every task
/// itself (the light path names no director).
#[test]
fn all_terminal_is_a_report_step() {
    let mut events = base();
    events.extend([
        ev(3, "director", task_created("t1", "feature", 1)),
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(5, "actor-t1-c1", delivered("t1", 1, "a", 1)),
        ev(6, "director", reviewed("t1", 1, Verdict::Accepted, vec![])),
    ]);
    let mut state = replay(&events);
    assert_eq!(step(&state, "node report"), Some((6, "director".into())));
    // The director's report discharges it.
    apply(
        &mut state,
        &ev(7, "director", report("producer", "integration")),
    );
    assert!(step(&state, "node report").is_none());
}

/// The light path: every task cast by the producer itself — the
/// report step is the producer's own.
#[test]
fn all_terminal_on_the_light_path_is_the_producers() {
    let mut events = base();
    events.extend([
        ev(3, "producer", task_created("t1", "fix", 1)),
        ev(4, "producer", cast("t1", "actor-b", "b")),
        ev(5, "actor-t1-c1", delivered("t1", 1, "b", 1)),
        ev(6, "producer", reviewed("t1", 1, Verdict::Accepted, vec![])),
    ]);
    let state = replay(&events);
    assert_eq!(step(&state, "node report"), Some((6, "producer".into())));
}

/// A report handed to the producer is owed onward to the user —
/// keyed on the delivered report, owed from its delivery.
#[test]
fn a_delivered_report_is_the_producers_to_relay() {
    let mut events = base();
    events.extend([
        ev(3, "director", task_created("t1", "feature", 1)),
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(5, "actor-t1-c1", delivered("t1", 1, "a", 1)),
        ev(6, "director", reviewed("t1", 1, Verdict::Accepted, vec![])),
        ev(7, "director", report("producer", "integration")),
    ]);
    let mut state = replay(&events);
    // Still queued to the producer: not owed yet.
    assert!(step(&state, "report to user").is_none());
    apply(
        &mut state,
        &ev(8, "stage", EventKind::MessageDelivered { target: 7 }),
    );
    assert_eq!(step(&state, "report to user"), Some((7, "producer".into())));
    assert_eq!(
        next_steps(&state)
            .iter()
            .find(|s| s.label == "report to user")
            .unwrap()
            .since_ms,
        8000
    );
    apply(
        &mut state,
        &ev(9, "producer", report("user", "integration")),
    );
    assert!(step(&state, "report to user").is_none());
    // And now only the user's own step is left.
    assert_eq!(step(&state, "finish"), Some((9, "user".into())));
}

/// All tasks closed and the user told: `studio finish` is the
/// user's own — shown, never the stage's to push.
#[test]
fn reported_to_the_user_is_the_users() {
    let mut events = base();
    events.extend([
        ev(3, "director", task_created("t1", "feature", 1)),
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(5, "actor-t1-c1", delivered("t1", 1, "a", 1)),
        ev(6, "director", reviewed("t1", 1, Verdict::Accepted, vec![])),
        ev(7, "director", report("producer", "integration")),
        ev(8, "stage", EventKind::MessageDelivered { target: 7 }),
        ev(9, "producer", report("user", "integration")),
    ]);
    let state = replay(&events);
    let steps = next_steps(&state);
    assert_eq!(steps.len(), 1, "{steps:?}");
    assert_eq!(steps[0].label, "finish");
    assert_eq!(steps[0].owner, "user");
}

/// A `purpose: "nudge"` message is never the phase's command —
/// the writer's step survives one aimed at it.
#[test]
fn a_nudge_is_no_call() {
    let mut events = base();
    events.extend([
        ev(3, "producer", plan_request()),
        ev(
            4,
            "stage",
            EventKind::MessageQueued {
                to: "writer".into(),
                file: Some("messages/4-nudge.md".into()),
                purpose: Some("nudge".into()),
                source_seq: Some(3),
            },
        ),
    ]);
    let state = replay(&events);
    assert_eq!(
        step(&state, "plan and acceptance list"),
        Some((3, "writer".into()))
    );
}

/// Once the studio is finished nothing is owed — even a step that
/// stood a moment before.
#[test]
fn nothing_after_finished() {
    let mut events = base();
    events.extend([
        ev(3, "director", task_created("t1", "feature", 1)),
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(5, "actor-t1-c1", delivered("t1", 1, "a", 1)),
        ev(6, "director", reviewed("t1", 1, Verdict::Accepted, vec![])),
        ev(7, "producer", report("user", "integration")),
        ev(
            8,
            "user",
            EventKind::StudioFinished {
                reason: "done".into(),
            },
        ),
    ]);
    let state = replay(&events);
    assert!(next_steps(&state).is_empty());
}

/// Paused suspends everything: a paused task owes no step.
#[test]
fn a_paused_task_owes_nothing() {
    let mut events = base();
    events.extend([
        ev(3, "director", task_created("t1", "feature", 1)),
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(
            5,
            "actor-t1-c1",
            task_state("t1", TaskStateChange::Paused, "holding"),
        ),
    ]);
    let state = replay(&events);
    assert!(next_steps(&state).is_empty());
}

/// While the cast declares checks the review step waits for the
/// stage's records — nothing is owed before them — and is then owed
/// from the last `task.checked`, so a nudge can never overtake the
/// delivery notice the stage queues on the same records.
#[test]
fn a_review_step_waits_on_the_last_check() {
    let mut kind = studio_kind();
    let EventKind::StudioCreated { checks, .. } = &mut kind else {
        unreachable!()
    };
    *checks = vec![
        check_decl("unit", "cargo test --lib", 600),
        check_decl("lint", "cargo clippy", 60),
    ];
    let mut events = base();
    events[0] = ev(1, "user", kind);
    events.extend([
        ev(3, "director", task_created("t1", "feature", 1)),
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(5, "actor-t1-c1", delivered("t1", 1, "a", 1)),
    ]);
    let checked = |seq: u64, name: &str| {
        ev(
            seq,
            "stage",
            EventKind::TaskChecked {
                task: "t1".into(),
                delivery: 1,
                name: name.into(),
                exit_code: Some(0),
                timed_out: false,
                output: format!("checks/5-{name}.log"),
                duration_ms: 10,
            },
        )
    };
    // Nothing recorded: no review step, no matter how long it idles.
    let mut state = replay(&events);
    assert!(step(&state, "review t1").is_none());
    // One of two recorded: still nothing.
    apply(&mut state, &checked(6, "unit"));
    assert!(step(&state, "review t1").is_none());
    // Both recorded: owed from the last `task.checked` (seq 7).
    apply(&mut state, &checked(7, "lint"));
    assert_eq!(step(&state, "review t1"), Some((7, "director".into())));
    assert_eq!(
        next_steps(&state)
            .iter()
            .find(|s| s.label == "review t1")
            .unwrap()
            .since_ms,
        7000
    );
}

/// S9e: a task that still waits on dependencies owes no `cast` step —
/// nobody is nudged for a cast they may not do. Once the dependency
/// is accepted the step comes back, owed by the task's creator.
#[test]
fn a_waiting_task_owes_no_cast_step() {
    let mut events = base();
    events.push(ev(3, "director", task_created("t1", "feature", 1)));
    let mut dep = task_created("t2", "fix", 1);
    let EventKind::TaskCreated { dependencies, .. } = &mut dep else {
        unreachable!()
    };
    *dependencies = vec!["t1".into()];
    events.push(ev(4, "director", dep));
    let mut state = replay(&events);
    assert_eq!(step(&state, "cast t2"), None);
    // The dependency still waiting: t1's own cast step is owed, but
    // t2's is not.
    assert!(step(&state, "cast t1").is_some());
    events.extend([
        ev(5, "director", cast("t1", "actor-a", "a")),
        ev(6, "actor-t1-c1", delivered("t1", 1, "a", 1)),
        ev(7, "director", reviewed("t1", 1, Verdict::Accepted, vec![])),
    ]);
    state = replay(&events);
    assert_eq!(step(&state, "cast t2"), Some((4, "director".into())));
}

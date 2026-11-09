//! Validation-rule tests: task state changes, delivery/clear links,
//! and correction targets — each accepted and refused.

use std::collections::BTreeMap;

use crate::event::*;
use crate::state::{StudioState, TaskStatus, apply, replay};
use crate::testkit::*;
use crate::validate::{Refusal, validate};

#[test]
fn task_state_transition_rules() {
    let state = running_state();
    let pause = EventKind::TaskState {
        task: "t1".into(),
        state: TaskStateChange::Paused,
        reason: "hold".into(),
    };
    assert_eq!(validate(&state, &new_ev("director", pause)), Ok(()));
    // Resume refuses on a task that is not paused.
    let resume = EventKind::TaskState {
        task: "t1".into(),
        state: TaskStateChange::Resumed,
        reason: "go".into(),
    };
    assert_eq!(
        validate(&state, &new_ev("director", resume)),
        Err(Refusal::BadPauseResume {
            task: "t1".into(),
            state: "resumed",
            status: TaskStatus::Running
        })
    );
    // Abandon works from any state, but not twice.
    let abandon = EventKind::TaskState {
        task: "t1".into(),
        state: TaskStateChange::Abandoned,
        reason: "dropped".into(),
    };
    assert_eq!(
        validate(&state, &new_ev("director", abandon.clone())),
        Ok(())
    );
    let gone = state_with(&base_events(), vec![("director", abandon)]);
    let again = EventKind::TaskState {
        task: "t1".into(),
        state: TaskStateChange::Abandoned,
        reason: "again".into(),
    };
    assert_eq!(
        validate(&gone, &new_ev("director", again)),
        Err(Refusal::Terminal {
            task: "t1".into(),
            status: TaskStatus::Abandoned
        })
    );
}

#[test]
fn delivered_and_cleared_links_validate() {
    let mut base = base_events();
    base.push(ev(
        4,
        "producer",
        EventKind::MessageQueued {
            to: "director".into(),
            file: None,
            purpose: None,
            source_seq: None,
        },
    ));
    base.push(ev(
        5,
        "stage",
        EventKind::AttentionRaised {
            role: "actor".into(),
            reason: "stalled".into(),
            detail: String::new(),
        },
    ));
    let state = replay(&base);
    assert_eq!(
        validate(
            &state,
            &new_ev("stage", EventKind::MessageDelivered { target: 4 })
        ),
        Ok(())
    );
    assert_eq!(
        validate(
            &state,
            &new_ev("stage", EventKind::MessageDelivered { target: 5 })
        ),
        Err(Refusal::NoMessage(5))
    );
    assert_eq!(
        validate(
            &state,
            &new_ev("stage", EventKind::MessageDelivered { target: 99 })
        ),
        Err(Refusal::NoMessage(99))
    );
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "stage",
                EventKind::AttentionCleared {
                    target: 5,
                    reason: None
                }
            )
        ),
        Ok(())
    );
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "stage",
                EventKind::AttentionCleared {
                    target: 4,
                    reason: None
                }
            )
        ),
        Err(Refusal::NoAttention(4))
    );
    // Double delivery / double clear refuse.
    let mut s = state;
    apply(
        &mut s,
        &ev(6, "stage", EventKind::MessageDelivered { target: 4 }),
    );
    apply(
        &mut s,
        &ev(
            7,
            "stage",
            EventKind::AttentionCleared {
                target: 5,
                reason: None,
            },
        ),
    );
    assert_eq!(
        validate(
            &s,
            &new_ev("stage", EventKind::MessageDelivered { target: 4 })
        ),
        Err(Refusal::MessageDone(4))
    );
    assert_eq!(
        validate(
            &s,
            &new_ev(
                "stage",
                EventKind::AttentionCleared {
                    target: 5,
                    reason: None
                }
            )
        ),
        Err(Refusal::AttentionDone(5))
    );
}

/// `task.finding` records on any task that has the delivery —
/// delivered, running or already closed — and never touches status.
#[test]
fn task_finding_rules() {
    // Delivered task takes the finding.
    let state = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("actor", delivered("t1", 1, "a", 1)),
        ],
    );
    let f = task_finding("t1", 1, vec![finding("D1", Cause::DeliveryDefect)]);
    assert_eq!(validate(&state, &new_ev("producer", f.clone())), Ok(()));
    // A delivery the task does not have is refused.
    let bad = task_finding("t1", 9, vec![finding("D2", Cause::DeliveryDefect)]);
    assert_eq!(
        validate(&state, &new_ev("producer", bad)),
        Err(Refusal::NoDelivery {
            task: "t1".into(),
            delivery: 9
        })
    );
    // An unknown task is refused.
    let gone = task_finding("t9", 1, vec![finding("D3", Cause::DeliveryDefect)]);
    assert_eq!(
        validate(&state, &new_ev("producer", gone)),
        Err(Refusal::NoTask("t9".into()))
    );
    // After acceptance the finding still records — and the status
    // does not move.
    let mut closed = state;
    apply(
        &mut closed,
        &ev(9, "director", reviewed("t1", 1, Verdict::Accepted, vec![])),
    );
    assert_eq!(closed.tasks["t1"].status, TaskStatus::Accepted);
    assert_eq!(validate(&closed, &new_ev("producer", f.clone())), Ok(()));
    apply(&mut closed, &ev(10, "producer", f));
    let t = &closed.tasks["t1"];
    assert_eq!(t.status, TaskStatus::Accepted);
    assert_eq!(t.finding_reports.len(), 1);
    assert_eq!(t.finding_reports[0].delivery, 1);
    assert_eq!(t.finding_reports[0].by, "producer");
    assert_eq!(t.finding_reports[0].findings[0].id, "D1");
}

/// `role.session` records only on a role the log knows.
#[test]
fn role_session_needs_a_recorded_role() {
    let mut base = base_events();
    base.push(ev(
        4,
        "stage",
        EventKind::RoleStarted {
            role: "producer".into(),
            terminal: Some("t3".into()),
            actor: None,
            session_id: None,
            reason: None,
        },
    ));
    let state = replay(&base);
    let known = EventKind::RoleSession {
        role: "producer".into(),
        session_id: "s-1".into(),
    };
    assert_eq!(validate(&state, &new_ev("stage", known)), Ok(()));
    let unknown = EventKind::RoleSession {
        role: "actor-t9-c1".into(),
        session_id: "s-2".into(),
    };
    assert_eq!(
        validate(&state, &new_ev("stage", unknown)),
        Err(Refusal::NoRole("actor-t9-c1".into()))
    );
    // No studio at all is refused before the role check.
    let no_studio = EventKind::RoleSession {
        role: "producer".into(),
        session_id: "s-1".into(),
    };
    assert_eq!(
        validate(&StudioState::default(), &new_ev("stage", no_studio)),
        Err(Refusal::NoStudio)
    );
}

#[test]
fn correction_needs_a_known_target_and_field() {
    let state = running_state();
    let mut set = BTreeMap::new();
    set.insert("title".to_string(), "renamed".to_string());
    // t1's task.created is seq 3 in base_events.
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "producer",
                EventKind::Correction {
                    target: 3,
                    reason: "typo".into(),
                    set: set.clone()
                }
            )
        ),
        Ok(())
    );
    // Unknown seq.
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "producer",
                EventKind::Correction {
                    target: 77,
                    reason: "typo".into(),
                    set: set.clone()
                }
            )
        ),
        Err(Refusal::NoCorrectionTarget(77))
    );
    // Field the target does not have.
    let mut bad = BTreeMap::new();
    bad.insert("verdict".to_string(), "accepted".to_string());
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "producer",
                EventKind::Correction {
                    target: 3,
                    reason: "wrong".into(),
                    set: bad
                }
            )
        ),
        Err(Refusal::BadCorrectionKey {
            target: 3,
            key: "verdict".into()
        })
    );
    // Unparseable value.
    let mut badv = BTreeMap::new();
    badv.insert("requirement_version".to_string(), "many".to_string());
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "producer",
                EventKind::Correction {
                    target: 3,
                    reason: "wrong".into(),
                    set: badv
                }
            )
        ),
        Err(Refusal::BadCorrectionValue {
            target: 3,
            key: "requirement_version".into(),
            value: "many".into()
        })
    );
}

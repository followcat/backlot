//! Validation-rule tests: task state changes, delivery/clear links,
//! and correction targets — each accepted and refused.

use std::collections::BTreeMap;

use crate::event::*;
use crate::state::{TaskStatus, apply, replay};
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

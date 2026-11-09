//! Validation-rule tests: each task-flow rule accepted and refused.

use crate::event::*;
use crate::state::{StudioState, TaskStatus, replay};
use crate::testkit::*;
use crate::validate::{Refusal, validate};

#[test]
fn second_studio_created_is_refused() {
    let state = replay(&[ev(1, "user", studio_kind())]);
    assert_eq!(
        validate(&state, &new_ev("user", studio_kind())),
        Err(Refusal::StudioExists)
    );
    assert_eq!(
        validate(&StudioState::default(), &new_ev("user", studio_kind())),
        Ok(())
    );
}

#[test]
fn req_version_must_be_next() {
    let state = base_state();
    let good = EventKind::ReqVersion {
        version: 2,
        file: "req/v2.md".into(),
        summary: "more".into(),
    };
    assert_eq!(validate(&state, &new_ev("producer", good.clone())), Ok(()));
    let bad = EventKind::ReqVersion {
        version: 4,
        file: "req/v4.md".into(),
        summary: "skip".into(),
    };
    assert_eq!(
        validate(&state, &new_ev("producer", bad)),
        Err(Refusal::BadReqVersion {
            expected: 2,
            got: 4
        })
    );
    // No studio at all refuses everything but studio.created.
    assert_eq!(
        validate(&StudioState::default(), &new_ev("producer", good)),
        Err(Refusal::NoStudio)
    );
}

#[test]
fn plan_publish_needs_a_real_req_version() {
    let state = base_state();
    let kind = EventKind::PlanPublished {
        plan_kind: PlanKind::Plan,
        file: "plan/9-plan.md".into(),
        requirement_version: 9,
    };
    assert_eq!(
        validate(&state, &new_ev("writer", kind)),
        Err(Refusal::UnknownReqVersion(9))
    );
    let ok = EventKind::PlanPublished {
        plan_kind: PlanKind::Acceptance,
        file: "plan/9-acc.md".into(),
        requirement_version: 1,
    };
    assert_eq!(validate(&state, &new_ev("writer", ok)), Ok(()));
}

#[test]
fn task_created_checks_id_version_and_deps() {
    let state = running_state();
    // Duplicate id.
    assert_eq!(
        validate(&state, &new_ev("director", task_created("t1", "fix", 1))),
        Err(Refusal::TaskExists("t1".into()))
    );
    // Requirement version not yet recorded.
    assert_eq!(
        validate(&state, &new_ev("director", task_created("t3", "fix", 2))),
        Err(Refusal::UnknownReqVersion(2))
    );
    // Unknown dependency.
    let mut dep = task_created("t3", "fix", 1);
    if let EventKind::TaskCreated { dependencies, .. } = &mut dep {
        *dependencies = vec!["nope".into()];
    }
    assert_eq!(
        validate(&state, &new_ev("director", dep)),
        Err(Refusal::UnknownDependency {
            task: "t3".into(),
            dep: "nope".into()
        })
    );
    // Depends on an existing task: fine.
    let mut ok = task_created("t3", "fix", 1);
    if let EventKind::TaskCreated { dependencies, .. } = &mut ok {
        *dependencies = vec!["t1".into(), "t2".into()];
    }
    assert_eq!(validate(&state, &new_ev("director", ok)), Ok(()));
}

/// R11: cast only from the studio's candidates.
#[test]
fn cast_only_from_candidates() {
    let state = running_state();
    // Unknown candidate name.
    assert_eq!(
        validate(&state, &new_ev("director", cast("t2", "actor-z", "z"))),
        Err(Refusal::UnknownCandidate("actor-z".into()))
    );
    // Candidate name known but the identity doesn't match it.
    assert_eq!(
        validate(&state, &new_ev("director", cast("t2", "actor-a", "b"))),
        Err(Refusal::CandidateMismatch {
            candidate: "actor-a".into()
        })
    );
    assert_eq!(
        validate(&state, &new_ev("director", cast("t2", "actor-b", "b"))),
        Ok(())
    );
}

#[test]
fn cast_refused_on_paused_and_terminal_tasks() {
    let mut kinds = vec![
        ("actor", delivered("t1", 1, "a", 1)),
        ("director", reviewed("t1", 1, Verdict::Accepted, vec![])),
    ];
    let state = state_with(&base_events(), kinds.clone());
    assert_eq!(
        validate(&state, &new_ev("director", cast("t1", "actor-b", "b"))),
        Err(Refusal::Terminal {
            task: "t1".into(),
            status: TaskStatus::Accepted
        })
    );
    kinds.pop();
    kinds.push((
        "director",
        EventKind::TaskState {
            task: "t1".into(),
            state: TaskStateChange::Paused,
            reason: "hold".into(),
        },
    ));
    let paused = state_with(&base_events(), kinds);
    assert_eq!(
        validate(&paused, &new_ev("director", cast("t1", "actor-b", "b"))),
        Err(Refusal::Paused("t1".into()))
    );
}

/// Deliver only on a running task; numbers increase by one.
#[test]
fn deliver_only_on_running_task_in_order() {
    let state = running_state();
    // t2 is only created, not running.
    assert_eq!(
        validate(&state, &new_ev("actor", delivered("t2", 1, "b", 1))),
        Err(Refusal::NotRunning {
            task: "t2".into(),
            status: TaskStatus::Created
        })
    );
    // First delivery must be number 1.
    assert_eq!(
        validate(&state, &new_ev("actor", delivered("t1", 2, "a", 1))),
        Err(Refusal::BadDeliveryNumber {
            task: "t1".into(),
            expected: 1,
            got: 2
        })
    );
    assert_eq!(
        validate(&state, &new_ev("actor", delivered("t1", 1, "a", 1))),
        Ok(())
    );

    // While delivered, a second delivery refuses until review returns it.
    let delivered_state = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("actor", delivered("t1", 1, "a", 1)),
        ],
    );
    assert_eq!(
        validate(
            &delivered_state,
            &new_ev("actor", delivered("t1", 2, "a", 1))
        ),
        Err(Refusal::NotRunning {
            task: "t1".into(),
            status: TaskStatus::Delivered
        })
    );
    // After changes_requested, the task runs again and wants delivery 2.
    let rework = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("actor", delivered("t1", 1, "a", 1)),
            (
                "director",
                reviewed("t1", 1, Verdict::ChangesRequested, vec![]),
            ),
        ],
    );
    assert_eq!(
        validate(&rework, &new_ev("actor", delivered("t1", 2, "a", 1))),
        Ok(())
    );
}

/// Each delivery carries the requirement version it was dispatched under.
#[test]
fn delivery_carries_the_pinned_req_version() {
    let state = running_state();
    assert_eq!(
        validate(&state, &new_ev("actor", delivered("t1", 1, "a", 2))),
        Err(Refusal::ReqPinMismatch {
            task: "t1".into(),
            pinned: 1,
            got: 2
        })
    );
}

/// Review only the latest delivery of a delivered task.
#[test]
fn review_only_latest_delivery_of_delivered_task() {
    let state = running_state();
    assert_eq!(
        validate(
            &state,
            &new_ev("director", reviewed("t1", 1, Verdict::Accepted, vec![]))
        ),
        Err(Refusal::NotDelivered {
            task: "t1".into(),
            status: TaskStatus::Running
        })
    );
    let delivered = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("actor", delivered("t1", 1, "a", 1)),
            (
                "director",
                reviewed("t1", 1, Verdict::ChangesRequested, vec![]),
            ),
            ("actor", delivered("t1", 2, "a", 1)),
        ],
    );
    // Only delivery 2 is reviewable; delivery 1 is history.
    assert_eq!(
        validate(
            &delivered,
            &new_ev("director", reviewed("t1", 1, Verdict::Accepted, vec![]))
        ),
        Err(Refusal::NotLatestDelivery {
            task: "t1".into(),
            latest: 2,
            got: 1
        })
    );
    assert_eq!(
        validate(
            &delivered,
            &new_ev("director", reviewed("t1", 2, Verdict::Accepted, vec![]))
        ),
        Ok(())
    );
}

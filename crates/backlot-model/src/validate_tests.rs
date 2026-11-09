//! Validation-rule tests: each task-flow rule accepted and refused.

use crate::event::*;
use crate::state::{StudioState, TaskStatus, apply, replay};
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

/// The changes_requested that crosses the cap is accepted, and the
/// task lands at `at_cap` for the producer to resolve.
#[test]
fn cap_crossing_review_is_recorded_and_caps() {
    let state = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("actor", delivered("t1", 1, "a", 1)),
            (
                "director",
                reviewed("t1", 1, Verdict::ChangesRequested, vec![]),
            ),
            ("actor", delivered("t1", 2, "a", 1)),
            (
                "director",
                reviewed("t1", 2, Verdict::ChangesRequested, vec![]),
            ),
            ("actor", delivered("t1", 3, "a", 1)),
        ],
    );
    assert_eq!(state.tasks["t1"].rework_rounds, 2);
    let third = new_ev(
        "director",
        reviewed("t1", 3, Verdict::ChangesRequested, vec![]),
    );
    assert_eq!(validate(&state, &third), Ok(()));
    let mut s = state;
    apply(&mut s, &ev(30, "director", third.kind));
    assert_eq!(s.tasks["t1"].status, TaskStatus::AtCap);
    // At cap the producer can still recast or abandon — both allowed.
    assert_eq!(
        validate(&s, &new_ev("director", cast("t1", "actor-b", "b"))),
        Ok(())
    );
    let abandon = EventKind::TaskState {
        task: "t1".into(),
        state: TaskStateChange::Abandoned,
        reason: "rescoped".into(),
    };
    assert_eq!(validate(&s, &new_ev("producer", abandon)), Ok(()));
    // No delivery is possible at cap, and a review only accepts.
    assert_eq!(
        validate(&s, &new_ev("actor", delivered("t1", 4, "b", 1))),
        Err(Refusal::NotRunning {
            task: "t1".into(),
            status: TaskStatus::AtCap
        })
    );
    assert_eq!(
        validate(
            &s,
            &new_ev(
                "director",
                reviewed("t1", 3, Verdict::ChangesRequested, vec![])
            )
        ),
        Err(Refusal::AtCapVerdict { task: "t1".into() })
    );
    assert_eq!(
        validate(
            &s,
            &new_ev("director", reviewed("t1", 3, Verdict::Blocked, vec![]))
        ),
        Err(Refusal::AtCapVerdict { task: "t1".into() })
    );
    // Accept and partially-accept close the task from at_cap — still
    // only on the latest delivery.
    assert_eq!(
        validate(
            &s,
            &new_ev("producer", reviewed("t1", 2, Verdict::Accepted, vec![]))
        ),
        Err(Refusal::NotLatestDelivery {
            task: "t1".into(),
            latest: 3,
            got: 2
        })
    );
    assert_eq!(
        validate(
            &s,
            &new_ev("producer", reviewed("t1", 3, Verdict::Accepted, vec![]))
        ),
        Ok(())
    );
    assert_eq!(
        validate(
            &s,
            &new_ev(
                "producer",
                reviewed("t1", 3, Verdict::PartiallyAccepted, vec![])
            )
        ),
        Ok(())
    );
}

/// A recast restarts the cap count: `rework_rounds` keeps the task's
/// total but the producer's budget counts from the latest cast.
#[test]
fn recast_restarts_the_cap_count() {
    // cap = 2: three changes land at_cap.
    let mut s = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("actor", delivered("t1", 1, "a", 1)),
            (
                "director",
                reviewed("t1", 1, Verdict::ChangesRequested, vec![]),
            ),
            ("actor", delivered("t1", 2, "a", 1)),
            (
                "director",
                reviewed("t1", 2, Verdict::ChangesRequested, vec![]),
            ),
            ("actor", delivered("t1", 3, "a", 1)),
            (
                "director",
                reviewed("t1", 3, Verdict::ChangesRequested, vec![]),
            ),
        ],
    );
    assert_eq!(s.tasks["t1"].status, TaskStatus::AtCap);
    // The recast runs the task again; one more changes-requested is
    // round 1 of the new budget, not past the cap.
    apply(&mut s, &ev(30, "director", cast("t1", "actor-b", "b")));
    assert_eq!(s.tasks["t1"].status, TaskStatus::Running);
    apply(&mut s, &ev(31, "actor", delivered("t1", 4, "b", 1)));
    let fourth = new_ev(
        "director",
        reviewed("t1", 4, Verdict::ChangesRequested, vec![]),
    );
    assert_eq!(validate(&s, &fourth), Ok(()));
    apply(&mut s, &ev(32, "director", fourth.kind));
    let t = &s.tasks["t1"];
    assert_eq!(t.rework_rounds, 4); // the task's total
    assert_eq!(t.status, TaskStatus::Running); // not at cap: 1 of 2
    // Two more rounds under actor-b hit the cap again.
    apply(&mut s, &ev(33, "actor", delivered("t1", 5, "b", 1)));
    apply(
        &mut s,
        &ev(
            34,
            "director",
            reviewed("t1", 5, Verdict::ChangesRequested, vec![]),
        ),
    );
    apply(&mut s, &ev(35, "actor", delivered("t1", 6, "b", 1)));
    apply(
        &mut s,
        &ev(
            36,
            "director",
            reviewed("t1", 6, Verdict::ChangesRequested, vec![]),
        ),
    );
    let t = &s.tasks["t1"];
    assert_eq!(t.rework_rounds, 6);
    assert_eq!(t.status, TaskStatus::AtCap);
}

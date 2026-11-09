//! Rework-cap tests: reaching the cap, what still validates at the
//! cap, and a recast restarting the producer's budget.

use crate::event::*;
use crate::state::{TaskStatus, apply};
use crate::testkit::*;
use crate::validate::{Refusal, validate};

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

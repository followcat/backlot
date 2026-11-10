//! The per-task budget (S9e): one `over budget` raise for a running
//! or delivered task past it — a recast does not raise a second —
//! and a `done` clear once the task is terminal.

use std::collections::BTreeMap;

use backlot_model::event::*;
use backlot_model::state::*;
use backlot_model::testkit::*;

use super::*;

const STUDIO: &str = "acme-shop";

/// t1 cast at seq 5 (ts 5000) under a 2-second budget: over from
/// now > 7000.
fn budgeted() -> Vec<Event> {
    let mut kind = studio_kind();
    let EventKind::StudioCreated { task_budget_s, .. } = &mut kind else {
        unreachable!()
    };
    *task_budget_s = Some(2);
    vec![
        ev(1, "user", kind),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "s".into(),
            },
        ),
        ev(3, "director", task_created("t1", "feature", 1)),
        ev(5, "director", cast("t1", "actor-a", "a")),
    ]
}

fn raised(seq: u64, role: &str) -> Event {
    ev(
        seq,
        "stage",
        EventKind::AttentionRaised {
            role: role.into(),
            reason: "over budget".into(),
            detail: "task t1 is over budget: 3s used of 2s".into(),
            needs_user: false,
        },
    )
}

fn over_budget(actions: &[Action]) -> Vec<&Action> {
    actions
        .iter()
        .filter(|a| matches!(a, Action::Attention { reason, .. } if reason == "over budget"))
        .collect()
}

/// Under the budget nothing raises; past it one attention lands on
/// the task's current actor key, naming task, budget and used.
#[test]
fn an_over_budget_task_raises_once_on_its_actor() {
    let state = replay(&budgeted());
    let quiet = reconcile(STUDIO, &state, &BTreeMap::new(), 7_000);
    assert!(over_budget(&quiet).is_empty(), "{quiet:?}");
    let acts = reconcile(STUDIO, &state, &BTreeMap::new(), 8_000);
    let [
        Action::Attention {
            role,
            reason,
            detail,
        },
    ] = over_budget(&acts)[..]
    else {
        panic!("wanted one over-budget raise: {acts:?}")
    };
    assert_eq!(role, "actor-t1-c1");
    assert_eq!(reason, "over budget");
    for needle in ["t1", "2s", "3s"] {
        assert!(detail.contains(needle), "{detail}");
    }
}

/// The recorded raise settles the task for good: a later tick does
/// not re-raise, and a recast — a new actor key — does not either.
#[test]
fn a_recast_does_not_raise_a_second() {
    let mut events = budgeted();
    events.extend([
        raised(6, "actor-t1-c1"),
        ev(7, "director", cast("t1", "actor-b", "b")),
    ]);
    let state = replay(&events);
    let acts = reconcile(STUDIO, &state, &BTreeMap::new(), 20_000);
    assert!(over_budget(&acts).is_empty(), "{acts:?}");
}

/// A terminal task clears its open raise `done`; a running one
/// keeps it.
#[test]
fn terminal_clears_done_running_keeps() {
    let mut events = budgeted();
    events.push(raised(6, "actor-t1-c1"));
    let state = replay(&events);
    let acts = reconcile(STUDIO, &state, &BTreeMap::new(), 9_000);
    assert!(
        !acts
            .iter()
            .any(|a| matches!(a, Action::ClearAttention { target: 6, .. })),
        "{acts:?}"
    );
    // Delivered and accepted: terminal → the clear lands.
    let mut events = budgeted();
    events.extend([
        raised(6, "actor-t1-c1"),
        ev(7, "actor-t1-c1", delivered("t1", 1, "a", 1)),
        ev(8, "director", reviewed("t1", 1, Verdict::Accepted, vec![])),
    ]);
    let state = replay(&events);
    let acts = reconcile(STUDIO, &state, &BTreeMap::new(), 9_000);
    assert!(
        acts.iter()
            .any(|a| matches!(a, Action::ClearAttention { target: 6, reason } if reason == "done")),
        "{acts:?}"
    );
}

/// A delivered task over its budget raises too — delivered is the
/// task still costing time.
#[test]
fn a_delivered_task_over_budget_raises() {
    let mut events = budgeted();
    events.push(ev(6, "actor-t1-c1", delivered("t1", 1, "a", 1)));
    let state = replay(&events);
    let acts = reconcile(STUDIO, &state, &BTreeMap::new(), 9_000);
    assert_eq!(over_budget(&acts).len(), 1, "{acts:?}");
}

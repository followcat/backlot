//! `used_ms` arithmetic (S9e): the task's first actor
//! `role.started` to close or now, minus every `paused` → `resumed`
//! stretch. A cast that still waits — held back by a runner cap —
//! has used nothing.

use crate::budget::used_ms;
use crate::event::*;
use crate::state::replay;
use crate::testkit::*;

fn studio() -> Vec<Event> {
    vec![
        ev(1, "user", studio_kind()),
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
    ]
}

/// `ev(seq, …)` carries `ts_ms = seq * 1000`: the actor starts at
/// 5000, pause 6000 → resume 7000, a second pause at 8000 still
/// open at now — 7000 of wall minus 1000 + 4000 paused leaves 2000.
#[test]
fn paused_stretches_are_left_out() {
    let mut events = studio();
    events.extend([
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(5, "stage", role_started("actor-t1-c1")),
        ev(6, "user", task_state("t1", TaskStateChange::Paused, "hold")),
        ev(7, "user", task_state("t1", TaskStateChange::Resumed, "go")),
        ev(
            8,
            "user",
            task_state("t1", TaskStateChange::Paused, "again"),
        ),
    ]);
    let state = replay(&events);
    let t = &state.tasks["t1"];
    assert_eq!(used_ms(t, 12_000), 2_000);
}

/// A terminal task counts to `closed_ts_ms`, not to now — and the
/// pause inside its span still comes out.
#[test]
fn a_closed_task_stops_counting_at_its_close() {
    let mut events = studio();
    events.extend([
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(5, "stage", role_started("actor-t1-c1")),
        ev(6, "user", task_state("t1", TaskStateChange::Paused, "hold")),
        ev(7, "user", task_state("t1", TaskStateChange::Resumed, "go")),
        ev(8, "actor-t1-c1", delivered("t1", 1, "a", 1)),
        ev(9, "director", reviewed("t1", 1, Verdict::Accepted, vec![])),
    ]);
    let state = replay(&events);
    let t = &state.tasks["t1"];
    // 5000 → 9000 = 4000 wall, 1000 paused → 3000 — `now` is ignored.
    assert_eq!(used_ms(t, 999_000), 3_000);
}

/// An uncast task has used nothing, and neither has a cast whose
/// actor never started — the wait under a cap is free. The clock
/// opens at the start, not the cast.
#[test]
fn a_cast_that_waits_has_used_nothing() {
    let state = replay(&studio());
    assert_eq!(used_ms(&state.tasks["t1"], 99_000), 0);
    let mut events = studio();
    events.extend([
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(6, "stage", role_started("actor-t1-c1")),
    ]);
    let state = replay(&events);
    // 6000 → 10000; the 2000 the cast sat waiting does not count.
    assert_eq!(used_ms(&state.tasks["t1"], 10_000), 4_000);
    // Without the start the same `now` reads zero.
    let mut held = studio();
    held.push(ev(4, "director", cast("t1", "actor-a", "a")));
    let state = replay(&held);
    assert_eq!(used_ms(&state.tasks["t1"], 10_000), 0);
}

/// A pause that ran before the actor's start — pausing a `created`
/// task — never reaches the counted span.
#[test]
fn a_pause_before_the_start_costs_nothing() {
    let mut events = studio();
    events.extend([
        ev(4, "user", task_state("t1", TaskStateChange::Paused, "hold")),
        ev(5, "user", task_state("t1", TaskStateChange::Resumed, "go")),
        ev(6, "director", cast("t1", "actor-a", "a")),
        ev(7, "stage", role_started("actor-t1-c1")),
    ]);
    let state = replay(&events);
    let t = &state.tasks["t1"];
    assert_eq!(used_ms(t, 9_000), 2_000);
}

/// A recast does not rewind the clock: the first actor's start
/// stays the origin even when a later cast's actor starts too.
#[test]
fn a_recast_keeps_the_first_start() {
    let mut events = studio();
    events.extend([
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(5, "stage", role_started("actor-t1-c1")),
        ev(6, "director", cast("t1", "actor-b", "b")),
        ev(7, "stage", role_started("actor-t1-c2")),
    ]);
    let state = replay(&events);
    let t = &state.tasks["t1"];
    assert_eq!(used_ms(t, 10_000), 5_000);
}

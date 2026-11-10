//! `used_ms` arithmetic (S9e): first cast to close or now, minus
//! every `paused` → `resumed` stretch.

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

/// `ev(seq, …)` carries `ts_ms = seq * 1000`: cast at 4000, pause
/// 5000 → resume 6000, a second pause at 7000 still open at now —
/// 8000 of wall minus 1000 + 5000 paused leaves 2000 used.
#[test]
fn paused_stretches_are_left_out() {
    let mut events = studio();
    events.extend([
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(5, "user", task_state("t1", TaskStateChange::Paused, "hold")),
        ev(6, "user", task_state("t1", TaskStateChange::Resumed, "go")),
        ev(
            7,
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
        ev(5, "user", task_state("t1", TaskStateChange::Paused, "hold")),
        ev(6, "user", task_state("t1", TaskStateChange::Resumed, "go")),
        ev(7, "actor-t1-c1", delivered("t1", 1, "a", 1)),
        ev(8, "director", reviewed("t1", 1, Verdict::Accepted, vec![])),
    ]);
    let state = replay(&events);
    let t = &state.tasks["t1"];
    // 4000 → 8000 = 4000 wall, 1000 paused → 3000 — `now` is ignored.
    assert_eq!(used_ms(t, 999_000), 3_000);
}

/// An uncast task has used nothing; a running one counts its whole
/// span while it was never paused.
#[test]
fn the_uncast_and_the_unpaused() {
    let state = replay(&studio());
    assert_eq!(used_ms(&state.tasks["t1"], 99_000), 0);
    let mut events = studio();
    events.push(ev(4, "director", cast("t1", "actor-a", "a")));
    let state = replay(&events);
    assert_eq!(used_ms(&state.tasks["t1"], 10_000), 6_000);
}

/// A pause that ran before the first cast — pausing a `created`
/// task — never reaches the counted span.
#[test]
fn a_pause_before_the_first_cast_costs_nothing() {
    let mut events = studio();
    events.extend([
        ev(4, "user", task_state("t1", TaskStateChange::Paused, "hold")),
        ev(5, "user", task_state("t1", TaskStateChange::Resumed, "go")),
        ev(6, "director", cast("t1", "actor-a", "a")),
    ]);
    let state = replay(&events);
    let t = &state.tasks["t1"];
    assert_eq!(used_ms(t, 8_000), 2_000);
}

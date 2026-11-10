//! The staleness table (S9d): a queued notice is dropped at send
//! time when the event its `source_seq` names no longer stands —
//! one test per rule row, plus the never-dropped rows.

use backlot_model::event::*;
use backlot_model::state::{Message, apply, replay};
use backlot_model::testkit::*;

use crate::stale::stale_reason;

/// A queued stage notice whose body is `messages/<source>-<label>.md`.
fn notice(seq: u64, source_seq: u64, label: &str) -> Message {
    Message {
        seq,
        from: "stage".into(),
        to: "director".into(),
        file: Some(format!("messages/{source_seq}-{label}.md")),
        purpose: if label == "nudge" {
            Some("nudge".into())
        } else {
            None
        },
        source_seq: Some(source_seq),
        delivered: false,
        dropped: false,
        ts_ms: 0,
        delivered_ms: None,
    }
}

fn attention(role: &str, reason: &str) -> EventKind {
    EventKind::AttentionRaised {
        role: role.into(),
        reason: reason.into(),
        detail: "the screen".into(),
        needs_user: false,
    }
}

/// `attention`, `stalled`, `lost` and `owed` notices all die the same
/// death: their source is an `attention.raised`, and once it clears
/// the notice has nothing left to say.
#[test]
fn an_attention_notice_is_stale_once_the_attention_clears() {
    let mut log = base_events();
    log.push(ev(
        4,
        "stage",
        attention("actor-t1-c1", "permission prompt"),
    ));
    let open = replay(&log);
    for label in ["attention", "stalled", "lost", "owed"] {
        assert_eq!(
            stale_reason(&open, &notice(9, 4, label)),
            None,
            "{label} while open"
        );
    }
    log.push(ev(
        5,
        "stage",
        EventKind::AttentionCleared {
            target: 4,
            reason: Some("sent enter".into()),
        },
    ));
    let cleared = replay(&log);
    for label in ["attention", "stalled", "lost", "owed"] {
        assert_eq!(
            stale_reason(&cleared, &notice(9, 4, label)),
            Some("attention cleared"),
            "{label} after the clear"
        );
    }
}

/// A `delivered` notice dies three ways: the delivery got a review,
/// a newer delivery exists, or the task closed — checked in that
/// order, since a review usually comes with the rest.
#[test]
fn a_delivered_notice_drops_once_the_delivery_moves_on() {
    let base = base_events();
    let state = state_with(
        &base,
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("actor-t1-c1", delivered("t1", 1, "a", 1)), // seq 5
        ],
    );
    let m = notice(9, 5, "delivered");
    assert_eq!(stale_reason(&state, &m), None, "awaiting review");

    // The review lands: `delivery reviewed`.
    let mut seen = state.clone();
    apply(
        &mut seen,
        &ev(6, "director", reviewed("t1", 1, Verdict::Accepted, vec![])),
    );
    assert_eq!(stale_reason(&seen, &m), Some("delivery reviewed"));

    // A second delivery with no review on the first (the reducer is
    // permissive — validation keeps ledgers honest): `superseded`.
    let mut twice = state.clone();
    apply(
        &mut twice,
        &ev(6, "actor-t1-c1", delivered("t1", 2, "a", 1)),
    );
    assert_eq!(stale_reason(&twice, &m), Some("superseded"));

    // Closed without a review — an abandonment: `task closed`.
    let mut closed = state.clone();
    apply(
        &mut closed,
        &ev(
            6,
            "director",
            task_state("t1", TaskStateChange::Abandoned, "cut"),
        ),
    );
    assert_eq!(stale_reason(&closed, &m), Some("task closed"));

    // Reviewed wins over superseded and closed when all three hold.
    let mut all = state.clone();
    apply(
        &mut all,
        &ev(
            6,
            "director",
            reviewed("t1", 1, Verdict::ChangesRequested, vec![]),
        ),
    );
    apply(&mut all, &ev(7, "actor-t1-c1", delivered("t1", 2, "a", 1)));
    assert_eq!(stale_reason(&all, &m), Some("delivery reviewed"));
}

/// A `rework` notice dies when a newer delivery exists (`superseded`)
/// or the task left `running` for any reason (`no longer running`).
#[test]
fn a_rework_notice_drops_when_the_round_moves_on() {
    let base = base_events();
    let state = state_with(
        &base,
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("actor-t1-c1", delivered("t1", 1, "a", 1)),
            (
                "director",
                reviewed("t1", 1, Verdict::ChangesRequested, vec![]), // seq 6
            ),
        ],
    );
    let m = notice(9, 6, "rework");
    assert_eq!(stale_reason(&state, &m), None, "actor owes the rework");

    // The rework came in: `superseded`.
    let mut again = state.clone();
    apply(
        &mut again,
        &ev(7, "actor-t1-c1", delivered("t1", 2, "a", 1)),
    );
    assert_eq!(stale_reason(&again, &m), Some("superseded"));

    // Paused mid-rework: `no longer running`. A terminal task reads
    // the same — it is not running either.
    let mut paused = state.clone();
    apply(
        &mut paused,
        &ev(7, "user", task_state("t1", TaskStateChange::Paused, "hold")),
    );
    assert_eq!(stale_reason(&paused, &m), Some("no longer running"));
    let mut abandoned = state.clone();
    apply(
        &mut abandoned,
        &ev(
            7,
            "director",
            task_state("t1", TaskStateChange::Abandoned, "cut"),
        ),
    );
    assert_eq!(stale_reason(&abandoned, &m), Some("no longer running"));
}

/// An `at-cap` notice lives only while the task sits at the cap.
#[test]
fn an_at_cap_notice_drops_when_the_task_leaves_the_cap() {
    let mut kinds: Vec<(&str, EventKind)> = vec![
        ("director", cast("t1", "actor-a", "a")),
        ("actor-t1-c1", delivered("t1", 1, "a", 1)),
        (
            "director",
            reviewed("t1", 1, Verdict::ChangesRequested, vec![]),
        ),
        ("actor-t1-c1", delivered("t1", 2, "a", 1)),
        (
            "director",
            reviewed("t1", 2, Verdict::ChangesRequested, vec![]),
        ),
    ];
    // The cap is 2: this third changes-requested puts t1 at_cap.
    kinds.push(("actor-t1-c1", delivered("t1", 3, "a", 1)));
    kinds.push((
        "director",
        reviewed("t1", 3, Verdict::ChangesRequested, vec![]), // seq 10
    ));
    let state = state_with(&base_events(), kinds);
    assert_eq!(
        state.tasks["t1"].status,
        backlot_model::state::TaskStatus::AtCap
    );
    let m = notice(20, 10, "at-cap");
    assert_eq!(stale_reason(&state, &m), None, "producer owes the call");

    let mut moved = state.clone();
    apply(
        &mut moved,
        &ev(
            11,
            "producer",
            task_state("t1", TaskStateChange::Abandoned, "cut losses"),
        ),
    );
    assert_eq!(stale_reason(&moved, &m), Some("no longer at cap"));
}

/// A `blocked` notice lives only while the task is still blocked.
#[test]
fn a_blocked_notice_drops_when_the_task_unblocks() {
    let state = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            (
                "actor-t1-c1",
                task_state("t1", TaskStateChange::Blocked, "waiting on an answer"), // seq 5
            ),
        ],
    );
    let m = notice(9, 5, "blocked");
    assert_eq!(stale_reason(&state, &m), None, "still blocked");

    let mut resumed = state.clone();
    apply(
        &mut resumed,
        &ev(
            6,
            "director",
            task_state("t1", TaskStateChange::Resumed, "answered"),
        ),
    );
    assert_eq!(stale_reason(&resumed, &m), Some("no longer blocked"));
}

/// A `resumed` notice dies only when the task closes — a pause or a
/// new block leaves it true, since the resume still happened and the
/// actor still owes the work.
#[test]
fn a_resumed_notice_drops_only_on_a_terminal_task() {
    let state = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("user", task_state("t1", TaskStateChange::Paused, "hold")),
            (
                "director",
                task_state("t1", TaskStateChange::Resumed, "carry on"), // seq 6
            ),
        ],
    );
    let m = notice(9, 6, "resumed");
    assert_eq!(stale_reason(&state, &m), None, "running again");

    let mut paused = state.clone();
    apply(
        &mut paused,
        &ev(7, "user", task_state("t1", TaskStateChange::Paused, "hold")),
    );
    assert_eq!(stale_reason(&paused, &m), None, "paused is not closed");

    let mut closed = state.clone();
    apply(
        &mut closed,
        &ev(
            7,
            "director",
            task_state("t1", TaskStateChange::Abandoned, "cut"),
        ),
    );
    assert_eq!(stale_reason(&closed, &m), Some("task closed"));
}

/// A `nudge` dies when the step it names is no longer owed — the
/// step's key is the event seq `next_steps` carries.
#[test]
fn a_nudge_drops_once_the_step_is_done() {
    // t1 is created but uncast: `cast t1` is owed, keyed on the
    // task's creation event.
    let state = base_state();
    let steps = backlot_model::steps::next_steps(&state);
    let owed = steps.iter().find(|s| s.label == "cast t1").unwrap();
    let m = notice(9, owed.key, "nudge");
    assert_eq!(stale_reason(&state, &m), None, "cast still owed");

    let mut casted = state.clone();
    apply(&mut casted, &ev(4, "director", cast("t1", "actor-b", "b")));
    assert_eq!(stale_reason(&casted, &m), Some("step done"));

    // An unrelated seq — a nudge for a step that never was — reads
    // done already.
    assert_eq!(
        stale_reason(&state, &notice(9, 77, "nudge")),
        Some("step done")
    );
}

/// `recovered`, `denied` and `req` never drop — each is a fact, not
/// a pointer at live state — and a message with no `source_seq` (a
/// `tell`, an `ask`, a `dispatch`, the plan request) is never even
/// checked.
#[test]
fn the_never_stale_rows() {
    let mut log = base_events();
    // A cleared attention behind a `recovered` notice — the source
    // is gone and the notice still stands.
    log.push(ev(4, "stage", attention("director", "permission prompt")));
    log.push(ev(
        5,
        "stage",
        EventKind::AttentionCleared {
            target: 4,
            reason: None,
        },
    ));
    let state = replay(&log);
    for label in ["recovered", "denied", "req"] {
        assert_eq!(
            stale_reason(&state, &notice(9, 4, label)),
            None,
            "{label} is never stale"
        );
    }
    // No source: never checked, whatever the file is named.
    let mut tell = notice(9, 4, "attention");
    tell.source_seq = None;
    assert_eq!(stale_reason(&state, &tell), None);
    tell.file = None;
    tell.purpose = Some("dispatch".into());
    assert_eq!(stale_reason(&state, &tell), None);
    // An unknown label on a sourced file is left alone too — the
    // table drops only what it knows.
    assert_eq!(stale_reason(&state, &notice(9, 4, "mystery")), None);
}

/// `backlot status` counts a dropped message apart: not queued,
/// not delivered — its own count, shown only while any exist.
#[test]
fn status_counts_dropped_separately() {
    let mut state = base_state();
    let mut dropped_a = notice(4, 1, "attention");
    dropped_a.dropped = true;
    let mut dropped_b = notice(5, 2, "blocked");
    dropped_b.dropped = true;
    let mut sent = notice(6, 3, "delivered");
    sent.delivered = true;
    state.messages = vec![notice(7, 4, "req"), dropped_a, dropped_b, sent];
    let out = crate::render::status("acme-shop", &state, &Default::default(), 0);
    assert!(
        out.contains("messages: 1 queued, 1 delivered, 2 dropped"),
        "{out}"
    );

    state.messages.retain(|m| !m.dropped);
    let out = crate::render::status("acme-shop", &state, &Default::default(), 0);
    assert!(out.contains("messages: 1 queued, 1 delivered"), "{out}");
    assert!(!out.contains("dropped"), "{out}");
}

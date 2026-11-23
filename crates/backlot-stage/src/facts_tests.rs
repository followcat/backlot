//! The facts Hollywood is handed: an attention's class, its age and —
//! for a screen snapshot — when it was taken; each role's current live
//! work state; and the same facts in the notice body it reads. Before
//! this, `backlot status` printed a bare `reason — detail`, so a
//! launch-time guest sign-in snapshot read as the role's current state
//! (H1, P5).

use std::collections::BTreeMap;

use backlot_model::event::{ActorIdentity, Event, EventKind};
use backlot_model::state::{Attention, replay};
use backlot_model::summary::Work;
use backlot_model::testkit::*;

use crate::render::{attention_fact, status};

fn raised(seq: u64, role: &str, reason: &str, detail: &str) -> Event {
    ev(
        seq,
        "stage",
        EventKind::AttentionRaised {
            role: role.into(),
            reason: reason.into(),
            detail: detail.into(),
            needs_user: false,
        },
    )
}

/// An open `session id not found` attention — the stage's own
/// bookkeeping — is printed as `info`, aged, and never as a bare
/// problem line.
#[test]
fn an_info_attention_is_labelled_info_and_aged() {
    let mut events = base_events();
    // `testkit::ev` stamps `ts_ms = seq * 1000`, so seq 9 was raised at
    // 9 s; read two hours later it is `2h ago`.
    events.push(raised(
        9,
        "producer",
        "session id not found",
        "no conversation recorded",
    ));
    let state = replay(&events);
    let now = 2 * 60 * 60 * 1000 + 9_000; // two hours after the raise at 9 s
    let out = status(
        "acme-shop",
        &state,
        &Default::default(),
        now,
        &Default::default(),
    );
    assert!(
        out.contains(
            "producer: session id not found — no conversation recorded (info · raised 2h ago)"
        ),
        "{out}"
    );
    // The old bare form never appears without its class.
    assert!(
        !out.contains("producer: session id not found — no conversation recorded\n"),
        "{out}"
    );
}

/// A screen-snapshot attention carries the time it was taken and the
/// stale warning: `screen at <time>, may be stale`. A bare snapshot is
/// exactly what misled Hollywood into reporting agy's launch splash.
#[test]
fn a_snapshot_attention_carries_its_time_and_stale_warning() {
    let mut events = base_events();
    events.push(ev(
        9,
        "stage",
        EventKind::AttentionRaised {
            role: "producer".into(),
            reason: "permission prompt".into(),
            detail: "Bash: ls; needs the user; screen saved to attention/9.txt".into(),
            needs_user: true,
        },
    ));
    let state = replay(&events);
    // Read an hour after the raise at 9 s.
    let now = 60 * 60 * 1000 + 9_000;
    let out = status(
        "acme-shop",
        &state,
        &Default::default(),
        now,
        &Default::default(),
    );
    assert!(
        out.contains(
            "producer: permission prompt — Bash: ls; needs the user; screen saved to attention/9.txt (needs you · raised 1h ago · screen at "
        ),
        "{out}"
    );
    assert!(out.contains("may be stale)"), "{out}");
}

/// An attention with no saved screen is not labelled a snapshot — the
/// stale warning belongs only to a screen read.
#[test]
fn a_screeenless_attention_has_no_snapshot_note() {
    let a = Attention {
        seq: 5,
        role: "producer".into(),
        reason: "stalled".into(),
        detail: "no hook signal for 600s".into(),
        open: true,
        ts_ms: 0,
        needs_user: false,
        cleared_reason: None,
    };
    let fact = attention_fact(&a, 5_000);
    assert!(fact.starts_with("stuck · raised 5s ago"), "{fact}");
    assert!(!fact.contains("screen at"), "{fact}");
}

/// A live role's current work state rides its line, from the stage's
/// sample — not from any attention snapshot.
#[test]
fn a_role_line_carries_its_live_work_state() {
    let actor = ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "plain".into(),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    };
    let mut events = base_events();
    events.push(ev(
        9,
        "stage",
        EventKind::RoleStarted {
            role: "producer".into(),
            terminal: Some("bl-acme-shop-producer".into()),
            actor: Some(actor),
            session_id: Some("s-1".into()),
            reason: None,
        },
    ));
    let state = replay(&events);
    let live = BTreeMap::from([(
        ("acme-shop".to_string(), "producer".to_string()),
        Work::Working,
    )]);
    let out = status("acme-shop", &state, &Default::default(), 0, &live);
    assert!(
        out.contains("producer  runner-prod/model-prod/high/standard (live) · working"),
        "{out}"
    );

    // A `waiting` role reads `waiting`; a role with no sample keeps the
    // plain lifecycle word.
    let waiting = BTreeMap::from([(
        ("acme-shop".to_string(), "producer".to_string()),
        Work::Waiting,
    )]);
    let out = status("acme-shop", &state, &Default::default(), 0, &waiting);
    assert!(out.contains("(live) · waiting"), "{out}");
    let out = status(
        "acme-shop",
        &state,
        &Default::default(),
        0,
        &Default::default(),
    );
    assert!(
        out.contains("producer  runner-prod/model-prod/high/standard (live)\n"),
        "{out}"
    );
}

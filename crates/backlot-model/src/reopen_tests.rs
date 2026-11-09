//! Reopening a closed person (R19): `held` on the reduced state and
//! every `role.reopen`/`role.release` refusal.

use crate::event::*;
use crate::state::{RoleLifecycle, apply};
use crate::testkit::*;
use crate::validate::{Refusal, validate};

fn started(role: &str, terminal: &str, session: &str) -> EventKind {
    EventKind::RoleStarted {
        role: role.into(),
        terminal: Some(terminal.into()),
        actor: Some(actor("a")),
        session_id: Some(session.into()),
        reason: None,
    }
}

fn reopen(role: &str) -> EventKind {
    EventKind::RoleReopen { role: role.into() }
}

fn release(role: &str) -> EventKind {
    EventKind::RoleRelease { role: role.into() }
}

/// t1 cast; `actor-t1-c1` ran and was deliberately closed.
fn closed_actor() -> Vec<(&'static str, EventKind)> {
    vec![
        ("director", cast("t1", "actor-a", "a")),
        ("stage", started("actor-t1-c1", "bl-acme-shop-t1", "sess-1")),
        (
            "stage",
            EventKind::RoleClosed {
                role: "actor-t1-c1".into(),
                reason: Some("no longer desired".into()),
            },
        ),
    ]
}

/// `role.reopen` holds the person; `role.release` and `role.closed`
/// clear the hold.
#[test]
fn reopen_holds_and_release_or_close_lets_go() {
    let state = state_with(&base_events(), closed_actor());
    let mut state = state;
    apply(&mut state, &ev(9, "user", reopen("actor-t1-c1")));
    assert!(state.roles["actor-t1-c1"].held);
    assert_eq!(
        state.roles["actor-t1-c1"].lifecycle(),
        RoleLifecycle::Closed
    );

    apply(&mut state, &ev(10, "user", release("actor-t1-c1")));
    assert!(!state.roles["actor-t1-c1"].held);

    apply(&mut state, &ev(11, "user", reopen("actor-t1-c1")));
    apply(
        &mut state,
        &ev(
            12,
            "stage",
            EventKind::RoleClosed {
                role: "actor-t1-c1".into(),
                reason: Some("released".into()),
            },
        ),
    );
    assert!(!state.roles["actor-t1-c1"].held);
}

/// The hold survives the resume — it is what keeps a reopened role
/// desired once it is live again.
#[test]
fn a_resumed_role_stays_held() {
    let mut state = state_with(&base_events(), closed_actor());
    apply(&mut state, &ev(9, "user", reopen("actor-t1-c1")));
    apply(
        &mut state,
        &ev(10, "stage", started("actor-t1-c1", "t9", "sess-1")),
    );
    let r = &state.roles["actor-t1-c1"];
    assert!(r.held);
    assert_eq!(r.lifecycle(), RoleLifecycle::Live);
}

/// Each `role.reopen` refusal, in the order they are checked.
#[test]
fn reopen_refusals() {
    let state = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("stage", started("actor-t1-c1", "bl-acme-shop-t1", "sess-1")),
            // A live sibling on the same task takes the terminal name.
            ("stage", started("actor-t1-c2", "bl-acme-shop-t1", "sess-2")),
            // A live role with a session cannot be reopened either.
            ("stage", started("writer", "bl-acme-shop-writer", "sess-w")),
            // A closed role without a session has nothing to resume.
            (
                "stage",
                EventKind::RoleStarted {
                    role: "director".into(),
                    terminal: Some("bl-acme-shop-director".into()),
                    actor: Some(actor("d")),
                    session_id: None,
                    reason: None,
                },
            ),
            (
                "stage",
                EventKind::RoleClosed {
                    role: "director".into(),
                    reason: Some("stop".into()),
                },
            ),
        ],
    );
    let reopen_at = |role: &str| validate(&state, &new_ev("user", reopen(role)));
    assert_eq!(
        reopen_at("writer"),
        Err(Refusal::RoleNotClosed {
            role: "writer".into(),
            state: "live"
        })
    );
    assert_eq!(
        reopen_at("actor-t1-c2"),
        Err(Refusal::RoleNotClosed {
            role: "actor-t1-c2".into(),
            state: "live"
        })
    );
    assert_eq!(reopen_at("nobody"), Err(Refusal::NoRole("nobody".into())));
    assert_eq!(
        reopen_at("director"),
        Err(Refusal::RoleNoSession("director".into()))
    );

    // Close actor-t1-c1 and leave its sibling live: the terminal
    // name is taken.
    let state = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("stage", started("actor-t1-c1", "bl-acme-shop-t1", "sess-1")),
            (
                "stage",
                EventKind::RoleClosed {
                    role: "actor-t1-c1".into(),
                    reason: Some("stop".into()),
                },
            ),
            ("stage", started("actor-t1-c2", "bl-acme-shop-t1", "sess-2")),
        ],
    );
    assert_eq!(
        validate(&state, &new_ev("user", reopen("actor-t1-c1"))),
        Err(Refusal::RoleTerminalTaken {
            role: "actor-t1-c1".into(),
            other: "actor-t1-c2".into()
        })
    );
    // A live role on another task does not block it.
    let free = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("stage", started("actor-t1-c1", "bl-acme-shop-t1", "sess-1")),
            (
                "stage",
                EventKind::RoleClosed {
                    role: "actor-t1-c1".into(),
                    reason: Some("stop".into()),
                },
            ),
            ("stage", started("actor-t2-c1", "bl-acme-shop-t2", "sess-3")),
        ],
    );
    assert_eq!(
        validate(&free, &new_ev("user", reopen("actor-t1-c1"))),
        Ok(())
    );

    // Held is refused; a lost role reads its own state.
    let mut state = state;
    apply(&mut state, &ev(9, "user", reopen("actor-t1-c1")));
    assert_eq!(
        validate(&state, &new_ev("user", reopen("actor-t1-c1"))),
        Err(Refusal::RoleHeld("actor-t1-c1".into()))
    );
}

/// `role.release` wants a held role and nothing else.
#[test]
fn release_refusals() {
    let state = state_with(&base_events(), closed_actor());
    assert_eq!(
        validate(&state, &new_ev("user", release("actor-t1-c1"))),
        Err(Refusal::RoleNotHeld("actor-t1-c1".into()))
    );
    assert_eq!(
        validate(&state, &new_ev("user", release("nobody"))),
        Err(Refusal::NoRole("nobody".into()))
    );
    let mut state = state;
    apply(&mut state, &ev(9, "user", reopen("actor-t1-c1")));
    assert_eq!(
        validate(&state, &new_ev("user", release("actor-t1-c1"))),
        Ok(())
    );
    // A live held role releases too — the Stop path closes it.
}

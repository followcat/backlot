//! `role.restart` (L2): the request the stage carries out, its
//! refusals, and the `restarted` word the reduced state keeps.

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
        prompt_hash: None,
    }
}

fn restart(role: &str, fresh: bool) -> EventKind {
    EventKind::RoleRestart {
        role: role.into(),
        fresh,
        reason: None,
    }
}

/// A `role.restart` on a live role lands as a request on the reduced
/// state; the `role.closed` (reason `restarted`) that carries it out
/// keeps it, and the start that follows clears it and marks the role
/// restarted.
#[test]
fn a_restart_is_a_request_the_start_clears() {
    let mut state = state_with(
        &base_events(),
        vec![(
            "stage",
            started("producer", "bl-acme-shop-producer", "sess-p"),
        )],
    );
    assert_eq!(
        validate(&state, &new_ev("user", restart("producer", false))),
        Ok(())
    );
    apply(&mut state, &ev(9, "user", restart("producer", false)));
    let req = state.roles["producer"]
        .restart
        .clone()
        .expect("the request");
    assert_eq!(req.seq, 9);
    assert!(!req.fresh);

    // The deliberate stop the restart owns keeps the request.
    apply(
        &mut state,
        &ev(
            10,
            "stage",
            EventKind::RoleClosed {
                role: "producer".into(),
                reason: Some("restarted".into()),
            },
        ),
    );
    assert!(
        state.roles["producer"].restart.is_some(),
        "a restart's own close keeps the request"
    );

    // The start that follows clears it and stamps the restarted word.
    apply(
        &mut state,
        &ev(
            11,
            "stage",
            EventKind::RoleResumed {
                role: "producer".into(),
                terminal: Some("bl-acme-shop-producer".into()),
                actor: Some(actor("a")),
                session_id: Some("sess-p".into()),
                reason: Some("restarted".into()),
            },
        ),
    );
    let r = &state.roles["producer"];
    assert_eq!(r.lifecycle(), RoleLifecycle::Live);
    assert!(r.restart.is_none());
    assert_eq!(r.restarted_ms, Some(11_000));
    assert_eq!(r.session_id.as_deref(), Some("sess-p"));
}

/// A fresh restart's start is a `role.started` with the fresh word:
/// it keeps the request's session out of the state, a new
/// conversation.
#[test]
fn a_fresh_restart_starts_a_new_conversation() {
    let mut state = state_with(
        &base_events(),
        vec![(
            "stage",
            started("producer", "bl-acme-shop-producer", "sess-p"),
        )],
    );
    apply(&mut state, &ev(9, "user", restart("producer", true)));
    apply(
        &mut state,
        &ev(
            10,
            "stage",
            EventKind::RoleStarted {
                role: "producer".into(),
                terminal: Some("bl-acme-shop-producer".into()),
                actor: Some(actor("a")),
                session_id: Some("sess-new".into()),
                reason: Some("restarted".into()),
                prompt_hash: None,
            },
        ),
    );
    let r = &state.roles["producer"];
    assert_eq!(r.session_id.as_deref(), Some("sess-new"));
    assert_eq!(r.restarted_ms, Some(10_000));
    assert!(r.restart.is_none());
}

/// A plain close (not a restart's) ends a pending request.
#[test]
fn a_plain_close_drops_the_request() {
    let mut state = state_with(
        &base_events(),
        vec![(
            "stage",
            started("producer", "bl-acme-shop-producer", "sess-p"),
        )],
    );
    apply(&mut state, &ev(9, "user", restart("producer", false)));
    apply(
        &mut state,
        &ev(
            10,
            "stage",
            EventKind::RoleClosed {
                role: "producer".into(),
                reason: Some("no longer desired".into()),
            },
        ),
    );
    assert!(state.roles["producer"].restart.is_none());
}

/// A restart refuses a closed person (reopen is that) and a finished
/// studio.
#[test]
fn restart_refusals() {
    let mut state = state_with(
        &base_events(),
        vec![
            ("stage", started("writer", "bl-acme-shop-writer", "sess-w")),
            (
                "stage",
                EventKind::RoleClosed {
                    role: "writer".into(),
                    reason: Some("no more plans".into()),
                },
            ),
        ],
    );
    assert_eq!(
        validate(&state, &new_ev("user", restart("writer", false))),
        Err(Refusal::RestartClosed("writer".into()))
    );
    assert_eq!(
        validate(&state, &new_ev("user", restart("nobody", false))),
        Err(Refusal::NoRole("nobody".into()))
    );

    // Finished: the live producer cannot be restarted either.
    state = state_with(
        &base_events(),
        vec![
            (
                "stage",
                started("producer", "bl-acme-shop-producer", "sess-p"),
            ),
            (
                "producer",
                EventKind::StudioFinished {
                    reason: "done".into(),
                },
            ),
        ],
    );
    assert_eq!(
        validate(&state, &new_ev("user", restart("producer", false))),
        Err(Refusal::RestartFinished)
    );
}

/// Only the user or the director restarts.
#[test]
fn restart_is_the_user_or_the_directors() {
    let state = state_with(
        &base_events(),
        vec![(
            "stage",
            started("producer", "bl-acme-shop-producer", "sess-p"),
        )],
    );
    assert_eq!(
        validate(&state, &new_ev("actor-t1-c1", restart("producer", false))),
        Err(Refusal::RoleBound {
            role: "actor-t1-c1".into(),
            who: "the user or the director",
        })
    );
}

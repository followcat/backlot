//! `backlot reopen` / `backlot close` end to end: the events land
//! only for the user or the director, and the validation refusals
//! surface as exit-1 messages.

use std::path::Path;

use crate::cli::Env;
use crate::cli_testkit::*;
use crate::store::{self, Pending, StoreError};

/// A `role.started` then `role.closed` for `actor-t1-c1` — what the
/// stage writes when a cast person is released.
fn close_actor(env: &Env) {
    let dir = store::studio_dir(
        Path::new(env.get("BACKLOT_STATE_DIR").unwrap()),
        "acme-shop",
    );
    let actor = backlot_model::event::ActorIdentity {
        runner: "runner-a".into(),
        runner_version: None,
        model: "model-a".into(),
        effort: "high".into(),
        permission_profile: "worktree".into(),
    };
    for kind in [
        backlot_model::EventKind::RoleStarted {
            role: "actor-t1-c1".into(),
            terminal: Some("bl-acme-shop-t1".into()),
            actor: Some(actor.clone()),
            session_id: Some("sess-1".into()),
            reason: None,
        },
        backlot_model::EventKind::RoleClosed {
            role: "actor-t1-c1".into(),
            reason: Some("no longer desired".into()),
        },
        // One live role for the not-closed refusal.
        backlot_model::EventKind::RoleStarted {
            role: "writer".into(),
            terminal: Some("bl-acme-shop-writer".into()),
            actor: Some(actor),
            session_id: Some("sess-w".into()),
            reason: None,
        },
    ] {
        store::append(&dir, move |_, _| {
            Ok::<_, StoreError<crate::cli::Fail>>(Pending {
                event: backlot_model::NewEvent::new(backlot_model::EventBy::role("stage"), kind),
                files: Vec::new(),
            })
        })
        .unwrap();
    }
}

fn studio_with_closed_person(scratch: &Scratch, env: &Env) {
    let cast = fixture(scratch, "cast.toml", CAST_TOML);
    ok(
        env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    close_actor(env);
}

fn held(env: &Env) -> bool {
    let json = ok(env, "status --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    v["roles"]["actor-t1-c1"]["held"].as_bool().unwrap()
}

#[test]
fn reopen_holds_and_close_releases() {
    let (scratch, env) = scratch();
    studio_with_closed_person(&scratch, &env);
    assert!(!held(&env));

    let out = ok(&env, "reopen --person actor-t1-c1 --as user");
    assert!(out.contains("role.reopen actor-t1-c1"), "{out}");
    assert!(held(&env));

    let out = ok(&env, "close --person actor-t1-c1 --as director");
    assert!(out.contains("role.release actor-t1-c1"), "{out}");
    assert!(!held(&env));
}

#[test]
fn reopen_and_close_are_for_user_and_director() {
    let (scratch, env) = scratch();
    studio_with_closed_person(&scratch, &env);
    for cmd in ["reopen", "close"] {
        let msg = refused(
            &env,
            &format!("{cmd} --person actor-t1-c1 --as actor-t1-c1"),
        );
        assert!(msg.contains("the user or the director"), "{cmd}: {msg}");
    }
}

#[test]
fn reopen_refusals_reach_the_caller() {
    let (scratch, env) = scratch();
    studio_with_closed_person(&scratch, &env);
    assert!(usage(&env, "reopen --as user").contains("missing --person"));
    assert!(usage(&env, "close --as user").contains("missing --person"));
    assert!(refused(&env, "reopen --person nobody --as user").contains("no role 'nobody'"));
    assert!(
        refused(&env, "reopen --person writer --as user").contains("only a closed person"),
        "live role must refuse"
    );
    ok(&env, "reopen --person actor-t1-c1 --as user");
    assert!(
        refused(&env, "reopen --person actor-t1-c1 --as user").contains("already held"),
        "a held person cannot be reopened twice"
    );
    ok(&env, "close --person actor-t1-c1 --as user");
    assert!(
        refused(&env, "close --person actor-t1-c1 --as user").contains("not held"),
        "release wants a held role"
    );
}

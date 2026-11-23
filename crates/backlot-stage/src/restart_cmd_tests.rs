//! `backlot restart` end to end (L2): the ledger request lands for the
//! user or the director, the refusals reach the caller as plain
//! sentences, and `close` on a fixed seat says why instead of "held".

use crate::cli::Env;
use crate::cli_testkit::*;
use crate::store::{self, Pending, StoreError};

/// A `role.started` for `role` at `terminal` with `session`.
fn start_role(env: &Env, role: &str, terminal: &str, session: &str) {
    let dir = store::studio_dir(
        std::path::Path::new(env.get("BACKLOT_STATE_DIR").unwrap()),
        "acme-shop",
    );
    let actor = backlot_model::event::ActorIdentity {
        runner: "runner-a".into(),
        runner_version: None,
        model: "model-a".into(),
        effort: Some("high".into()),
        permission_profile: "worktree".into(),
    };
    store::append(&dir, move |_, _| {
        Ok::<_, StoreError<crate::cli::Fail>>(Pending {
            event: backlot_model::NewEvent::new(
                backlot_model::EventBy::role("stage"),
                backlot_model::EventKind::RoleStarted {
                    role: role.into(),
                    terminal: Some(terminal.into()),
                    actor: Some(actor),
                    session_id: Some(session.into()),
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
}

fn studio_with_live_roles(scratch: &Scratch, env: &Env) {
    let cast = fixture(scratch, "cast.toml", CAST_TOML);
    ok(
        env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    start_role(env, "producer", "bl-acme-shop-producer", "sess-p");
    start_role(env, "director", "bl-acme-shop-director", "sess-d");
}

/// `restart` records the request on the role; the ledger carries it
/// with `fresh` and the reason.
#[test]
fn restart_records_the_request() {
    let (scratch, env) = scratch();
    studio_with_live_roles(&scratch, &env);
    let out = ok(&env, "restart --person director --reason stuck --as user");
    assert!(out.contains("role.restart"), "{out}");
    let json = ok(&env, "status --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    // A resume restart carries no `fresh` flag and its reason.
    assert!(v["roles"]["director"]["restart"].is_object(), "{json}");
    assert_eq!(
        v["roles"]["director"]["restart"]["reason"], "stuck",
        "{json}"
    );

    let out = ok(&env, "restart --person producer --fresh --as director");
    assert!(out.contains("role.restart"), "{out}");
    let v: serde_json::Value = serde_json::from_str(&ok(&env, "status --json")).unwrap();
    assert_eq!(v["roles"]["producer"]["restart"]["fresh"], true);
}

/// `close` on a fixed seat refuses plainly — no "held" jargon — and
/// points at restart.
#[test]
fn close_on_a_fixed_seat_is_a_plain_sentence() {
    let (scratch, env) = scratch();
    studio_with_live_roles(&scratch, &env);
    for role in ["producer", "director"] {
        let msg = refused(&env, &format!("close --person {role} --as user"));
        assert_eq!(
            msg,
            format!("the {role} is a fixed seat — use restart, or finish the studio"),
            "{role}"
        );
        assert!(!msg.contains("held"), "no jargon: {msg}");
    }
}

/// `restart` refuses a closed person and is the user's or director's.
#[test]
fn restart_refusals_reach_the_caller() {
    let (scratch, env) = scratch();
    studio_with_live_roles(&scratch, &env);
    // A closed role — the writer was never started; `close` on it is
    // not held, so it refuses plainly; below we use a started actor.
    assert!(usage(&env, "restart --as user").contains("missing --person"));
    assert!(refused(&env, "restart --person nobody --as user").contains("no role 'nobody'"));
    let msg = refused(&env, "restart --person producer --as actor-t1-c1");
    assert!(msg.contains("the user or the director"), "{msg}");
}

/// A finished studio refuses a restart.
#[test]
fn restart_refuses_a_finished_studio() {
    let (scratch, env) = scratch();
    studio_with_live_roles(&scratch, &env);
    let req = fixture(&scratch, "req.md", "# req\n");
    ok(
        &env,
        &format!("req revise --file {req} --summary s --as producer"),
    );
    ok(&env, "studio finish --reason done --as user");
    let msg = refused(&env, "restart --person producer --as user");
    assert_eq!(msg, "the studio is finished — reopen or restore it first");
}

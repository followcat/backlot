//! `backlot approve` and `backlot studio finish` end to end: the
//! events land only for the roles allowed to record them, and the
//! model's refusals — no open permission prompt, open tasks,
//! already finished — surface as exit-1 messages.

use std::path::Path;

use crate::cli::Env;
use crate::cli_testkit::*;
use crate::store::{self, Pending, StoreError};

fn studio_dir(env: &Env) -> std::path::PathBuf {
    store::studio_dir(
        Path::new(env.get("BACKLOT_STATE_DIR").unwrap()),
        "acme-shop",
    )
}

/// A stage-recorded `attention.raised` the way the permission-prompt
/// episode produces it.
fn raise(env: &Env, role: &str, reason: &str) {
    let dir = studio_dir(env);
    let (role, reason) = (role.to_string(), reason.to_string());
    store::append(&dir, move |_, _| {
        Ok::<_, StoreError<crate::cli::Fail>>(Pending {
            event: backlot_model::NewEvent::new(
                backlot_model::EventBy::role("stage"),
                backlot_model::EventKind::AttentionRaised {
                    role,
                    reason,
                    detail: "screen text".into(),
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
}

fn studio(scratch: &Scratch, env: &Env) {
    let cast = fixture(scratch, "cast.toml", CAST_TOML);
    ok(
        env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
}

#[test]
fn approve_records_the_decision() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    raise(&env, "actor-t1-c1", "permission prompt");

    let out = ok(
        &env,
        "approve --person actor-t1-c1 --decision allow --as director",
    );
    assert!(out.contains("role.approval"), "{out}");
    let out = ok(
        &env,
        "approve --person actor-t1-c1 --decision deny --as user",
    );
    assert!(out.contains("role.approval"), "{out}");

    let json = ok(&env, "status --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let decisions: Vec<&str> = v["approvals"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["decision"].as_str())
        .collect();
    assert_eq!(decisions, ["allow", "deny"], "{json}");
}

#[test]
fn approve_needs_an_open_permission_prompt() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let msg = refused(
        &env,
        "approve --person actor-t1-c1 --decision allow --as user",
    );
    assert!(msg.contains("no open permission prompt"), "{msg}");
    // A different role's open attention does not answer this one's.
    raise(&env, "actor-t2-c1", "permission prompt");
    let msg = refused(
        &env,
        "approve --person actor-t1-c1 --decision allow --as user",
    );
    assert!(msg.contains("no open permission prompt"), "{msg}");
    // Nor does another reason.
    raise(&env, "actor-t1-c1", "session id not found");
    let msg = refused(
        &env,
        "approve --person actor-t1-c1 --decision allow --as user",
    );
    assert!(msg.contains("no open permission prompt"), "{msg}");
}

#[test]
fn approve_is_for_user_and_director() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    raise(&env, "actor-t1-c1", "permission prompt");
    let msg = refused(
        &env,
        "approve --person actor-t1-c1 --decision allow --as actor-t1-c1",
    );
    assert!(msg.contains("the user or the director"), "{msg}");
    assert!(usage(&env, "approve --as user").contains("missing --person"));
    assert!(usage(&env, "approve --person actor-t1-c1 --as user").contains("missing --decision"));
    assert!(
        usage(
            &env,
            "approve --person actor-t1-c1 --decision maybe --as user"
        )
        .contains("bad --decision")
    );
}

#[test]
fn finish_refuses_with_open_tasks_and_names_them() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let req = fixture(&scratch, "req.md", "a small shop\n");
    ok(
        &env,
        &format!("req revise --file {req} --summary first --as producer"),
    );
    let acceptance = fixture(&scratch, "accept.md", "- renders\n");
    ok(
        &env,
        &format!(
            "task new --title front-page --kind feature --acceptance-file {acceptance} --as director"
        ),
    );
    let msg = refused(&env, "studio finish --as producer");
    assert!(msg.contains("open tasks"), "{msg}");
    assert!(msg.contains("t1"), "{msg}");
    // Abandoning the task makes it terminal; finish then lands.
    ok(&env, "task abandon --task t1 --reason dropped --as user");
    let out = ok(&env, "studio finish --reason done --as producer");
    assert!(out.contains("studio.finished"), "{out}");
    let msg = refused(&env, "studio finish --as user");
    assert!(msg.contains("already finished"), "{msg}");
}

#[test]
fn finish_is_for_user_and_producer() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let msg = refused(&env, "studio finish --as director");
    assert!(msg.contains("the user or the producer"), "{msg}");
    ok(&env, "studio finish --as user");
}

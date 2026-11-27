//! `backlot approve` and `backlot studio finish` end to end: the
//! events land only for the roles allowed to record them, finishing
//! is the person's alone, and the model's refusals — no open
//! permission prompt, open tasks, already finished — surface as
//! exit-1 messages.

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
                    needs_user: false,
                    account: None,
                    reset_at_ms: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
}

/// Ledger lines in the studio's own log — a refused command appends
/// none.
fn event_lines(env: &Env) -> usize {
    std::fs::read_to_string(studio_dir(env).join("events.jsonl"))
        .unwrap()
        .lines()
        .count()
}

fn studio(scratch: &Scratch, env: &Env) {
    let cast = fixture(
        scratch,
        "cast.toml",
        &format!("design_gate = \"off\"\n{CAST_TOML}"),
    );
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

/// `approve --person hollywood` is the user's answer to a prompt on
/// Hollywood's own screen: it lands in Hollywood's ledger, never a
/// studio's, and no role may record it.
#[test]
fn approve_hollywood_records_in_its_own_ledger() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let out = ok(
        &env,
        "approve --person hollywood --decision allow --as user",
    );
    assert!(out.contains("role.approval"), "{out}");
    let hdir = store::hollywood_dir(Path::new(env.get("BACKLOT_STATE_DIR").unwrap()));
    let state = store::load_hstate(&hdir).unwrap();
    assert_eq!(state.approvals.len(), 1);
    assert_eq!(
        state.approvals[0].decision,
        backlot_model::event::ApprovalDecision::Allow
    );
    let msg = refused(
        &env,
        "approve --person hollywood --decision allow --as hollywood",
    );
    assert!(msg.contains("the user can"), "{msg}");
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

/// Finishing is the person's alone: every role — the producer
/// included — is refused with the same line before any state is
/// read, and nothing joins the ledger. An empty studio still refuses
/// this way, ahead of the `--force` guard.
#[test]
fn only_the_person_finishes_the_studio() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let before = event_lines(&env);
    for role in ["producer", "director", "actor-t1-c1"] {
        let msg = refused(&env, &format!("studio finish --as {role}"));
        assert_eq!(
            msg,
            "finishing the studio is the person's call; say in your report that it is ready, and they finish it from the lot",
            "{role}"
        );
    }
    // Hollywood is refused earlier still: inside a studio it may only
    // `tell` the producer, and a finish is no exception.
    let msg = refused(&env, "studio finish --as hollywood");
    assert!(msg.contains("cannot do this"), "{msg}");
    assert_eq!(
        event_lines(&env),
        before,
        "a refused finish wrote to the ledger"
    );
    let state = store::load_state(&studio_dir(&env)).unwrap();
    assert!(state.finished.is_none());
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
    let msg = refused(&env, "studio finish --as user");
    assert!(msg.contains("open tasks"), "{msg}");
    assert!(msg.contains("t1"), "{msg}");
    // Abandoning the task makes it terminal; the person's finish then
    // lands, reason optional.
    ok(&env, "task abandon --task t1 --reason dropped --as user");
    let out = ok(&env, "studio finish --as user");
    assert!(out.contains("studio.finished"), "{out}");
    let state = store::load_state(&studio_dir(&env)).unwrap();
    assert!(state.finished.is_some());
    let msg = refused(&env, "studio finish --as user");
    assert!(msg.contains("already finished"), "{msg}");
}

/// `backlot attention clear` resolves a still-open attention by
/// hand — the stage-restart leftover case (S12d). A cleared or
/// never-raised seq is refused, and only a person clears.
#[test]
fn attention_clear_resolves_an_open_attention() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    raise(&env, "actor-t1-c1", "runner not supported");
    // seq 1 is studio.created; the raise is seq 2.
    let out = ok(&env, "attention clear 2 --as user");
    assert!(out.contains("attention.cleared"), "{out}");
    // The attention is closed — a second clear is refused.
    let msg = refused(&env, "attention clear 2 --as user");
    assert!(msg.contains("no open attention 2"), "{msg}");
    // A seq that was never an attention is refused the same way.
    let msg = refused(&env, "attention clear 1 --as user");
    assert!(msg.contains("no open attention 1"), "{msg}");
    let msg = refused(&env, "attention clear 99 --as user");
    assert!(msg.contains("no open attention 99"), "{msg}");
    // Only a person clears — an actor may not. The second raise is
    // seq 4: the first clear landed its own event at seq 3.
    raise(&env, "actor-t1-c1", "runner not supported");
    let msg = refused(&env, "attention clear 4 --as actor-t1-c1");
    assert!(msg.contains("cannot do this"), "{msg}");
    // The director may.
    ok(&env, "attention clear 4 --as director");
}

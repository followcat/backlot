//! S9b part 1 — who may record what, through the CLI on a scratch
//! state dir. The milestone's reproductions — an actor accepting its
//! own delivery and correcting a director's verdict — now refuse, as
//! does every other off-role step. The gate is append-time only: a
//! log written before it still replays.

use std::fs;
use std::path::Path;

use backlot_model::event::{EventKind, Verdict};
use backlot_model::state::TaskStatus;
use backlot_model::testkit as tk;
use backlot_model::{EventBy, NewEvent};

use crate::cli::Env;
use crate::cli_testkit::*;
use crate::store::{self, studio_dir};

fn studio(scratch: &Scratch, env: &Env) {
    let cast = fixture(scratch, "cast.toml", CAST_TOML);
    ok(
        env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let req = fixture(scratch, "req.md", "# req\n");
    ok(
        env,
        &format!("req revise --file {req} --summary s --as producer"),
    );
}

/// `task <id>` created by the director and cast to `actor-a` — from
/// then on its current actor key is `actor-<id>-c1`.
fn cast_task(scratch: &Scratch, env: &Env, id: &str) {
    let acc = fixture(scratch, &format!("acc-{id}.md"), "# acc\n");
    ok(
        env,
        &format!("task new --title {id} --kind feature --acceptance-file {acc} --as director"),
    );
    ok(
        env,
        &format!("cast --task {id} --actor actor-a --as director"),
    );
}

fn delivered_task(scratch: &Scratch, env: &Env, id: &str) {
    cast_task(scratch, env, id);
    let rep = fixture(scratch, &format!("rep-{id}.md"), "report\n");
    ok(
        env,
        &format!("deliver --commits a..b --report {rep} --as actor-{id}-c1 --task {id}"),
    );
}

fn review_seq(env: &Env, task: &str) -> u64 {
    let json = ok(env, "status --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    v["tasks"][task]["reviews"][0]["seq"].as_u64().unwrap()
}

/// The first reproduction: the actor delivered, then accepted its
/// own delivery from the same terminal. The review now refuses and
/// the task stays delivered.
#[test]
fn an_actor_cannot_review_its_own_delivery() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    delivered_task(&scratch, &env, "t1");
    let msg = refused(
        &env,
        "review --task t1 --delivery 1 --verdict accepted --conclusion done --as actor-t1-c1",
    );
    assert_eq!(
        msg,
        "role 'actor-t1-c1' cannot do this; the user, the producer or the director can"
    );
    let json = ok(&env, "status --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["tasks"]["t1"]["status"], "delivered");
}

/// The second reproduction: on t2 the actor rewrote the director's
/// `changes-requested` as `accepted` with `backlot correct` — it is
/// refused now too, and nothing is appended.
#[test]
fn an_actor_cannot_correct_a_verdict() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    // A first task takes the t1 id slot; the second is t2.
    cast_task(&scratch, &env, "t1");
    delivered_task(&scratch, &env, "t2");
    ok(
        &env,
        "review --task t2 --delivery 1 --verdict changes-requested --conclusion fix --as director",
    );
    let seq = review_seq(&env, "t2");
    let msg = refused(
        &env,
        &format!("correct --event {seq} --reason flip --set verdict=accepted --as actor-t2-c1"),
    );
    assert_eq!(
        msg,
        "role 'actor-t2-c1' cannot do this; the user, the producer or the director can"
    );
    let json = ok(&env, "status --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["corrections"].as_array().unwrap().len(), 0);
}

/// The writer's whitelist is `plan.published` and `tell`/`ask`:
/// `review`, `block` and everything else refuse.
#[test]
fn the_writer_records_only_plans_and_messages() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    delivered_task(&scratch, &env, "t1");
    // Allowed: a published plan and a plain message.
    let plan = fixture(&scratch, "plan.md", "# plan\n");
    ok(
        &env,
        &format!("plan publish --kind plan --file {plan} --as writer"),
    );
    let note = fixture(&scratch, "note.md", "note\n");
    ok(&env, &format!("tell director --file {note} --as writer"));
    // Refused.
    for (args, who) in [
        (
            "review --task t1 --delivery 1 --verdict accepted --conclusion done --as writer",
            "the user, the producer or the director",
        ),
        (
            "block --reason stuck --task t1 --as writer",
            "the task's own actor or a person",
        ),
        (
            "dispatch --file {disp} --as writer",
            "the producer or the stage",
        ),
        (
            "approve --person writer --decision allow --as writer",
            "the user or the director",
        ),
    ] {
        let args = args.replace("{disp}", &fixture(&scratch, "d.md", "go\n"));
        let msg = refused(&env, &args);
        assert_eq!(
            msg,
            format!("role 'writer' cannot do this; {who} can"),
            "{args}"
        );
    }
}

/// An actor blocks its own task only — t2's `blocked` is refused,
/// and so is every step that is not deliver/block/message.
#[test]
fn an_actor_blocks_only_its_own_task() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    cast_task(&scratch, &env, "t1");
    cast_task(&scratch, &env, "t2");
    ok(&env, "block --reason stuck --task t1 --as actor-t1-c1");
    let msg = refused(&env, "block --reason stuck --task t2 --as actor-t1-c1");
    assert_eq!(
        msg,
        "role 'actor-t1-c1' cannot do this; the task's own actor or a person can"
    );
    let msg = refused(&env, "task pause --task t1 --reason hold --as actor-t1-c1");
    assert_eq!(
        msg,
        "role 'actor-t1-c1' cannot do this; the user, the producer or the director can"
    );
}

/// Only the current cast delivers: after a recast the old key is
/// just another role name, and a delivery for another task is
/// refused too.
#[test]
fn only_the_current_cast_delivers() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    delivered_task(&scratch, &env, "t1");
    cast_task(&scratch, &env, "t2");
    // A review sends t1 back to running; a recast names c2 current.
    ok(
        &env,
        "review --task t1 --delivery 1 --verdict changes-requested --conclusion fix --as director",
    );
    ok(
        &env,
        "cast --task t1 --actor actor-b --reason switch --as director",
    );
    let rep = fixture(&scratch, "r2.md", "report 2\n");
    let msg = refused(
        &env,
        &format!("deliver --commits a..b --report {rep} --as actor-t1-c1 --task t1"),
    );
    assert_eq!(
        msg,
        "role 'actor-t1-c1' cannot do this; the task's current actor or the user can"
    );
    let msg = refused(
        &env,
        &format!("deliver --commits a..b --report {rep} --as actor-t1-c2 --task t2"),
    );
    assert_eq!(
        msg,
        "role 'actor-t1-c2' cannot do this; the task's current actor or the user can"
    );
}

/// `task.delivered` from anyone who is not the task's current actor
/// is the user's alone — producer and director do not deliver, the
/// stage never does, and a bare `actor` is not a role key.
#[test]
fn only_the_actor_or_the_user_delivers() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    cast_task(&scratch, &env, "t1");
    let rep = fixture(&scratch, "r.md", "report\n");
    for role in ["producer", "director", "stage", "actor", "nobody"] {
        let msg = refused(
            &env,
            &format!("deliver --commits a..b --report {rep} --as {role} --task t1"),
        );
        assert_eq!(
            msg,
            format!("role '{role}' cannot do this; the task's current actor or the user can"),
            "{role}"
        );
    }
    ok(
        &env,
        &format!("deliver --commits a..b --report {rep} --as user --task t1"),
    );
}

/// `stage start` and `stage run` refuse a working role — the stage
/// is not a role's to run, pinned or claimed.
#[test]
fn the_stage_is_not_for_working_roles() {
    let (_scratch, env) = scratch();
    for role in ["actor-t1-c1", "writer"] {
        for sub in ["start", "run"] {
            let msg = refused(&env, &format!("stage {sub} --as {role}"));
            assert_eq!(
                msg,
                format!("role '{role}' cannot do this; the user, the producer or the director can"),
                "stage {sub} --as {role}"
            );
        }
    }
    // A pinned role terminal is refused the same way.
    let mut env = scratch().1;
    env.vars.insert("BACKLOT_ROLE".into(), "actor-t1-c1".into());
    let msg = refused(&env, "stage run");
    assert_eq!(
        msg,
        "role 'actor-t1-c1' cannot do this; the user, the producer or the director can"
    );
}

/// A log written before the role gate still replays: an
/// actor-authored `task.reviewed` from an old ledger loads and
/// folds — the rule is on append, never on replay.
#[test]
fn an_old_actor_authored_review_still_loads() {
    let (scratch, env) = scratch();
    let dir = studio_dir(&scratch.dir, "acme-shop");
    fs::create_dir_all(&dir).unwrap();
    let ev = |seq: u64, role: &str, kind: EventKind| {
        NewEvent::new(EventBy::role(role), kind).stamp(seq, seq * 1000)
    };
    let log = vec![
        ev(1, "user", tk::studio_kind()),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "first".into(),
            },
        ),
        ev(3, "director", tk::task_created("t1", "feature", 1)),
        ev(4, "director", tk::cast("t1", "actor-a", "a")),
        ev(5, "actor-t1-c1", tk::delivered("t1", 1, "a", 1)),
        ev(
            6,
            "actor-t1-c1",
            tk::reviewed("t1", 1, Verdict::Accepted, vec![]),
        ),
    ];
    write_log(&dir, &log);
    let state = store::load_state(&dir).unwrap();
    assert_eq!(state.tasks["t1"].status, TaskStatus::Accepted);
    assert_eq!(state.tasks["t1"].reviews[0].reviewer, "director");
    let out = ok(&env, "status");
    assert!(out.contains("accepted"), "{out}");
}

/// Serialize the events one per line — the old log's own format.
fn write_log(dir: &Path, log: &[backlot_model::Event]) {
    let mut text = String::new();
    for e in log {
        text.push_str(&serde_json::to_string(e).unwrap());
        text.push('\n');
    }
    fs::write(dir.join("events.jsonl"), text).unwrap();
}

//! `backlot cast --priority` and `backlot priority` end to end:
//! the flag lands on `task.cast`, the command records
//! `task.priority`, and only the user or the director may run it.

use crate::cli_testkit::*;

/// Studio with one requirement and one task `t1`.
fn studio_with_task(scratch: &Scratch, env: &crate::cli::Env) {
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
    let acc = fixture(scratch, "acc.md", "# acc\n");
    ok(
        env,
        &format!("task new --title cart --kind feature --acceptance-file {acc} --as director"),
    );
}

fn priority_of(env: &crate::cli::Env) -> String {
    let json = ok(env, "status --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    v["tasks"]["t1"]["priority"].as_str().unwrap().to_string()
}

#[test]
fn cast_priority_and_the_priority_command() {
    let (scratch, env) = scratch();
    studio_with_task(&scratch, &env);
    assert_eq!(priority_of(&env), "normal");

    // --priority on cast sets it at cast time.
    ok(
        &env,
        "cast --task t1 --actor actor-a --reason strongest --priority high --as director",
    );
    assert_eq!(priority_of(&env), "high");

    // The user and the director may both change it.
    let out = ok(&env, "priority --task t1 --level low --as user");
    assert!(out.contains("task.priority t1 low"), "{out}");
    assert_eq!(priority_of(&env), "low");
    ok(&env, "priority --task t1 --level normal --as director");
    assert_eq!(priority_of(&env), "normal");
}

#[test]
fn priority_is_refused_for_a_working_role() {
    let (scratch, env) = scratch();
    studio_with_task(&scratch, &env);
    let msg = refused(&env, "priority --task t1 --level high --as actor-t1-c1");
    assert!(msg.contains("the user or the director"), "{msg}");
    let msg = refused(&env, "priority --task t1 --level high --as writer");
    assert!(msg.contains("the user or the director"), "{msg}");
}

#[test]
fn priority_needs_a_task_and_a_level() {
    let (scratch, env) = scratch();
    studio_with_task(&scratch, &env);
    assert!(usage(&env, "priority --level high --as user").contains("missing task"));
    assert!(usage(&env, "priority --task t1 --as user").contains("missing --level"));
    assert!(
        usage(&env, "priority --task t1 --level urgent --as user").contains("bad level 'urgent'")
    );
    assert!(refused(&env, "priority --task t9 --level low --as user").contains("no task 't9'"));
    assert!(
        usage(
            &env,
            "cast --task t1 --actor actor-a --priority meh --as director"
        )
        .contains("bad level 'meh'")
    );
}

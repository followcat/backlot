//! S9e item 1 — dependencies hold the cast, end to end on a scratch
//! state dir and a real repo. An open dependency is the model's own
//! refusal and `backlot status` shows `waits for t1`; an accepted
//! delivery that is not on `backlot/<studio>/integration` yet is the
//! CLI's git check — `merge t1 into integration first` — until the
//! merge lands.

use crate::cli::Env;
use crate::cli_testkit::*;
use crate::store::studio_dir;

/// The studio t2 waits on: a shared repo, a t1 already delivered
/// (not yet reviewed), the `work/integration` area the stage would
/// have made, and t2 created `--depends-on t1`.
fn studio(scratch: &Scratch, env: &Env) {
    let repo = shared_repo(scratch);
    let cast = fixture(scratch, "cast.toml", CAST_TOML);
    ok(
        env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            repo.display()
        ),
    );
    let req = fixture(scratch, "req.md", "# req\n");
    ok(
        env,
        &format!("req revise --file {req} --summary s --as producer"),
    );
    let acc = fixture(scratch, "acc.md", "# acc\n");
    ok(
        env,
        &format!("task new --title one --kind feature --acceptance-file {acc} --as director"),
    );
    ok(env, "cast --task t1 --actor actor-a --as director");
    let work = shared_area(scratch, "t1");
    let a = commit_in(&work, "one");
    let b = commit_in(&work, "two");
    let rep = fixture(scratch, "r.md", "report\n");
    ok(
        env,
        &format!("deliver --commits {a}..{b} --report {rep} --as actor-t1-c1 --task t1"),
    );
    shared_area(scratch, "integration");
    ok(
        env,
        &format!(
            "task new --title two --kind feature --acceptance-file {acc} --depends-on t1 --as director"
        ),
    );
}

/// While t1 is only delivered — open, not accepted — the cast is the
/// model's refusal and the task's status line says what it waits for.
#[test]
fn an_open_dependency_holds_the_cast_and_the_status_names_it() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let msg = refused(&env, "cast --task t2 --actor actor-b --as director");
    assert!(msg.contains("t2"), "{msg}");
    assert!(msg.contains("t1"), "{msg}");
    let line = ok(&env, "status")
        .lines()
        .find(|l| l.trim_start().starts_with("t2"))
        .unwrap_or_else(|| panic!("no t2 line in status"))
        .to_string();
    assert!(line.contains("waits for t1"), "{line}");
}

/// t1 accepted but not merged: `merge t1 into integration first`
/// until `backlot/acme-shop/t1` lands on the integration branch.
#[test]
fn an_accepted_dependency_must_be_merged_before_the_cast() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    ok(
        &env,
        "review --task t1 --delivery 1 --verdict accepted --conclusion done --as director",
    );
    let msg = refused(&env, "cast --task t2 --actor actor-b --as director");
    assert!(msg.contains("merge t1 into integration first"), "{msg}");
    let integ = studio_dir(&scratch.dir, "acme-shop").join("work/integration");
    git(&integ, &["merge", "backlot/acme-shop/t1"]);
    ok(&env, "cast --task t2 --actor actor-b --as director");
}

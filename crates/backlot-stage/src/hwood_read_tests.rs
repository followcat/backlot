//! The cross-studio reads (R24): `status --all`,
//! `records --studio/--all` — every studio in one view, then
//! Hollywood's block. Shared fixtures come from `hwood_tests`.

use crate::cli_testkit::*;
use crate::hwood_tests::{argv_ok, hwood_env, running};

#[test]
fn status_all_covers_the_lot() {
    let (scratch, env) = scratch();
    running(&scratch, &env);
    ok(
        &env,
        "hollywood propose pause --studio acme-shop --task t1 --why looping --as hollywood",
    );
    argv_ok(
        &env,
        &["hollywood", "report", "one open card", "--as", "hollywood"],
    );

    let out = ok(&env, "status --all");
    assert!(out.contains("acme-shop"), "{out}");
    assert!(out.contains("hollywood"), "{out}");
    assert!(out.contains("one open card"), "{out}");
    assert!(out.contains("c1"), "{out}");

    let json = ok(&env, "status --all --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v["studios"]["acme-shop"].is_object(), "{json}");
    assert_eq!(v["hollywood"]["headline"], "one open card");
    // A studio pin works the same everywhere — Hollywood's own
    // shell carries no BACKLOT_STUDIO, so `--studio` picks freely.
    let lenv = hwood_env(&env);
    let one = ok(&lenv, "status --studio acme-shop");
    assert!(one.contains("acme-shop"), "{one}");
    let msg = refused(&lenv, "status --studio no-shop");
    assert!(msg.contains("no studio 'no-shop'"), "{msg}");
    assert!(usage(&lenv, "status --all --studio acme-shop").contains("--all"));
}

#[test]
fn records_scope_to_one_studio_or_all() {
    let (scratch, env) = scratch();
    running(&scratch, &env);
    // A delivery gives records something to count.
    let work = work_area(&scratch, "t1");
    let a = commit_in(&work, "one");
    let b = commit_in(&work, "two");
    let rep = fixture(&scratch, "rep.md", "report\n");
    ok(
        &env,
        &format!(
            "deliver --commits {a}..{b} --report {rep} --check tests=pass --as actor-t1-c1 --task t1"
        ),
    );
    // `--studio` scopes the read; `--all` covers the lot. actor-a's
    // runner name is the record's key either way.
    let lenv = hwood_env(&env);
    let out = ok(&lenv, "records --studio acme-shop");
    assert!(out.contains("runner-a"), "{out}");
    let out = ok(&lenv, "records --all");
    assert!(out.contains("runner-a"), "{out}");
    assert!(usage(&lenv, "records --all --studio acme-shop").contains("--all"));
    let msg = refused(&lenv, "records --studio no-shop");
    assert!(msg.contains("no studio 'no-shop'"), "{msg}");
}

//! `backlot studio finish` guards an empty studio (L5): a just-made
//! studio with no requirement and no tasks refuses unless `--force`.
//! Finishing clears the studio's open questions (L6) — the model side
//! is covered in `studio_exchange_tests`.

use crate::cli::Env;
use crate::cli_testkit::*;

fn empty_studio(scratch: &Scratch, env: &Env) {
    let cast = fixture(scratch, "cast.toml", CAST_TOML);
    ok(
        env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
}

/// An empty studio refuses the finish with the guard's sentence, and
/// `--force` finishes it.
#[test]
fn finishing_an_empty_studio_needs_force() {
    let (scratch, env) = scratch();
    empty_studio(&scratch, &env);
    let msg = refused(&env, "studio finish --reason done --as user");
    assert_eq!(
        msg,
        "nothing was built yet (no requirement, no tasks) — add --force to finish anyway"
    );
    let out = ok(&env, "studio finish --reason done --force --as user");
    assert!(out.contains("studio.finished"), "{out}");
    let json = ok(&env, "status --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v["finished"].is_object(), "{json}");
}

/// A studio with a requirement finishes without `--force` — the guard
/// is only for the empty case.
#[test]
fn a_studio_with_a_requirement_finishes() {
    let (scratch, env) = scratch();
    empty_studio(&scratch, &env);
    let req = fixture(&scratch, "req.md", "# req\n");
    ok(
        &env,
        &format!("req revise --file {req} --summary s --as producer"),
    );
    let out = ok(&env, "studio finish --reason done --as user");
    assert!(out.contains("studio.finished"), "{out}");
}

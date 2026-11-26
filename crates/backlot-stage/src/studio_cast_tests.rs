//! `backlot studio cast show|set` through the real CLI on a scratch
//! state dir ("Changing a studio after it starts"): the show → set →
//! show round trip, the diff a dry run prints, every runnable refusal,
//! and "nothing changes".

use crate::cli::Env;
use crate::cli_testkit::*;
use crate::store;

/// A cast whose runners all resolve on this machine — `studio cast set`
/// checks that every named runner has an adapter and launches, the same
/// runnable check `studio new --dry-run` runs. `CAST_TOML`'s placeholder
/// runner names are fine for `studio new` (which does not check them),
/// but `set` does.
pub(crate) const REAL_CAST: &str = r#"
rework_cap = 2

[producer]
runner = "fake"
model = "model-p"
effort = "high"
permission_profile = "standard"

[director]
runner = "fake"
model = "model-d"
effort = "high"
permission_profile = "standard"

[writer]
runner = "fake"
model = "model-w"
effort = "medium"
permission_profile = "standard"

[[candidates]]
name = "actor-a"
runner = "fake"
model = "model-a"
effort = "high"
permission_profile = "worktree"

[[candidates]]
name = "actor-b"
runner = "fake"
model = "model-b"
effort = "low"
permission_profile = "worktree"
"#;

fn studio(scratch: &Scratch, env: &Env) {
    let cast = fixture(scratch, "cast.toml", REAL_CAST);
    ok(
        env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
}

fn events(scratch: &Scratch) -> Vec<backlot_model::Event> {
    store::load_events(&store::studio_dir(&scratch.dir, "acme-shop"))
        .unwrap()
        .0
}

/// `REAL_CAST` with one extra candidate on `fake`, so `set` accepts it.
fn with_candidate(scratch: &Scratch, name: &str) -> String {
    fixture(
        scratch,
        "cast-set.toml",
        &format!(
            r#"{REAL_CAST}

[[candidates]]
name = "{name}"
runner = "fake"
model = "m"
effort = "high"
permission_profile = "worktree"
"#
        ),
    )
}

/// show → set → show: the printed cast is a cast file, setting the same
/// file back records nothing, and setting a changed one rounds back.
#[test]
fn show_set_show_round_trips() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let before = ok(&env, "studio cast show");
    // The printed cast parses through `studio new`'s rules: it is a
    // well-formed cast file.
    let path = fixture(&scratch, "shown.toml", &before);
    let parsed =
        crate::commands::read_cast(std::path::Path::new(&path)).expect("show prints a cast file");
    assert_eq!(parsed.producer.runner, "fake");
    assert_eq!(parsed.candidates.len(), 2);
    // Setting an unchanged file is refused — nothing changes.
    let msg = refused(&env, &format!("studio cast set --file {path}"));
    assert!(msg.contains("nothing changes"), "{msg}");
    // A changed file records one studio.cast and shows back.
    let changed = with_candidate(&scratch, "actor-c");
    let out = ok(&env, &format!("studio cast set --file {changed}"));
    assert!(out.contains("studio.cast"), "{out}");
    let after = ok(&env, "studio cast show");
    assert!(after.contains("name = \"actor-c\""), "{after}");
    let kinds: Vec<&str> = events(&scratch).iter().map(|e| e.kind.name()).collect();
    assert_eq!(kinds.last(), Some(&"studio.cast"), "{kinds:?}");
}

/// The dry run prints the diff and records nothing.
#[test]
fn the_dry_run_prints_the_diff_and_records_nothing() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let before = events(&scratch).len();
    let changed = with_candidate(&scratch, "actor-c");
    let out = ok(&env, &format!("studio cast set --file {changed} --dry-run"));
    assert!(out.contains("dry run"), "{out}");
    assert!(
        out.contains("candidate + actor-c (fake/m/high (worktree))"),
        "{out}"
    );
    assert_eq!(
        events(&scratch).len(),
        before,
        "the dry run recorded an event"
    );
    // `--json` prints the same structured.
    let json = ok(
        &env,
        &format!("studio cast set --file {changed} --dry-run --json"),
    );
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["diff"]["candidates"][0]["name"], "actor-c");
    assert_eq!(v["dry_run"], true);
}

/// `show --json` is the settings structured.
#[test]
fn show_json_is_the_settings() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let json = ok(&env, "studio cast show --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["roles"]["producer"]["runner"], "fake");
    assert_eq!(v["candidates"][0]["name"], "actor-a");
    assert_eq!(v["rework_cap"], 2);
}

/// Every runnable refusal reads the way `studio new --dry-run` does.
#[test]
fn the_runnable_refusals_hold() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    // A zero limit is a usage error — the cast file's own rule, read
    // before anything is recorded.
    let zero = fixture(
        &scratch,
        "zero.toml",
        &format!("{REAL_CAST}\n[limits]\nfake = 0\n"),
    );
    let msg = usage(&env, &format!("studio cast set --file {zero}"));
    assert!(msg.contains("limits.fake must be at least 1"), "{msg}");
    // An unsupported runner is refused before the ledger is read —
    // `check_runners` answers it, the same rule `studio new --dry-run`
    // runs.
    let bad = fixture(
        &scratch,
        "bad.toml",
        &REAL_CAST.replace(
            "name = \"actor-b\"\nrunner = \"fake\"",
            "name = \"actor-b\"\nrunner = \"runner-zzz\"",
        ),
    );
    let msg = refused(&env, &format!("studio cast set --file {bad}"));
    assert!(
        msg.contains("runner 'runner-zzz' is not supported"),
        "{msg}"
    );
    // A cap that cannot seat the fixed roles — the file parser's own
    // usage error, the same rule `studio new` applies.
    let unseated = fixture(
        &scratch,
        "unseated.toml",
        &format!("{REAL_CAST}\n[limits]\nfake = 2\n"),
    );
    let msg = usage(&env, &format!("studio cast set --file {unseated}"));
    assert!(
        msg.contains("leaves no slot for the writer or an actor"),
        "{msg}"
    );
    // Nothing was recorded.
    let kinds: Vec<&str> = events(&scratch).iter().map(|e| e.kind.name()).collect();
    assert!(!kinds.contains(&"studio.cast"), "{kinds:?}");
}

/// A usage error for each bad invocation.
#[test]
fn the_usage_lines_name_the_flags() {
    let (_scratch, env) = scratch();
    let help = ok(&env, "studio cast show --help");
    assert!(help.contains("studio cast show"), "{help}");
    assert!(help.contains("studio cast set --file"), "{help}");
    let msg = usage(&env, "studio cast set");
    assert!(msg.contains("missing --file"), "{msg}");
    let msg = usage(&env, "studio cast nope");
    assert!(msg.contains("unknown studio cast subcommand"), "{msg}");
}

/// A role terminal cannot change the cast.
#[test]
fn a_role_terminal_is_refused() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let changed = with_candidate(&scratch, "actor-c");
    let mut vars = env.vars.clone();
    vars.insert("BACKLOT_ROLE".into(), "producer".into());
    let role_env = Env { vars };
    let msg = refused(&role_env, &format!("studio cast set --file {changed}"));
    assert!(msg.contains("BACKLOT_ROLE is set"), "{msg}");
}

/// A finished studio refuses a cast change.
#[test]
fn a_finished_studio_is_refused() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    ok(&env, "studio finish --reason done --force --as user");
    let changed = with_candidate(&scratch, "actor-c");
    let msg = refused(&env, &format!("studio cast set --file {changed}"));
    assert!(msg.contains("finished"), "{msg}");
}

/// A fixed role that changes identity in the file is recast the same
/// way `backlot recast` does — one `role.recast` for that role, in the
/// same apply, before the `studio.cast`.
#[test]
fn a_moved_fixed_identity_becomes_a_recast() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    // The director moves onto `fake` with a new model; nothing else
    // changes.
    let moved = REAL_CAST.replace("model = \"model-d\"", "model = \"new-d\"");
    assert_ne!(moved, REAL_CAST);
    let file = fixture(&scratch, "moved.toml", &moved);
    let out = ok(&env, &format!("studio cast set --file {file}"));
    assert!(out.contains("recast director"), "{out}");
    let kinds: Vec<&str> = events(&scratch).iter().map(|e| e.kind.name()).collect();
    let tail = &kinds[kinds.len() - 2..];
    assert_eq!(tail, ["role.recast", "studio.cast"], "{kinds:?}");
    // The effective cast shows the new director.
    let shown = ok(&env, "studio cast show");
    assert!(
        shown.contains("[director]\nrunner = \"fake\"\nmodel = \"new-d\""),
        "{shown}"
    );
}

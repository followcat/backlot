//! `backlot studio list --archived` and `backlot studio restore` (L7,
//! M3): a deleted studio's ledger lists from `archive/` and comes
//! back as a finished studio, refusing a name already taken.

use std::fs;

use crate::cli::Env;
use crate::cli_testkit::*;
use crate::store;

/// A git repo at `path` with one commit.
fn git_repo(path: &std::path::Path) {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "-b", "main"]);
    fs::write(path.join("README.md"), "base\n").unwrap();
    git(path, &["add", "."]);
    git(path, &["commit", "-m", "base"]);
}

/// A finished studio named `name` with one requirement, then deleted
/// — archived to `archive/<name>-<stamp>/`.
fn archived_studio(scratch: &Scratch, env: &Env, repo_name: &str, name: &str) {
    let repo = scratch.files.join(repo_name);
    git_repo(&repo);
    let cast = fixture(scratch, &format!("cast-{name}.toml"), CAST_TOML);
    let req = fixture(scratch, &format!("req-{name}.md"), "# req\n");
    ok(
        env,
        &format!(
            "studio new --studio {name} --title {name} --repo {} --cast {cast}",
            repo.display()
        ),
    );
    ok(
        env,
        &format!("req revise --studio {name} --file {req} --summary shop --as producer"),
    );
    ok(
        env,
        &format!("studio finish --studio {name} --reason done --as user"),
    );
    ok(env, &format!("studio delete {name} --as user"));
}

/// `studio list` no longer shows the deleted studio; `--archived`
/// shows it with its stamp, repo and last requirement summary.
#[test]
fn archived_studios_list_after_delete() {
    let (scratch, env) = scratch();
    archived_studio(&scratch, &env, "repo-shop", "acme-shop");

    // The live list is empty.
    let live = ok(&env, "studio list");
    assert!(!live.contains("acme-shop"), "{live}");

    let json = ok(&env, "studio list --archived --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let rows = v["archived"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{json}");
    let row = &rows[0];
    assert!(
        row["name"].as_str().unwrap().starts_with("acme-shop-"),
        "{json}"
    );
    assert_eq!(row["studio"], "acme-shop", "{json}");
    assert!(
        !row["stamp"].as_str().unwrap().is_empty(),
        "the stamp is carried: {json}"
    );
    assert_eq!(
        row["repo"],
        scratch.files.join("repo-shop").display().to_string()
    );
    assert_eq!(row["requirement"], "shop", "{json}");

    let table = ok(&env, "studio list --archived");
    for want in ["acme-shop", "shop", "repo-shop"] {
        assert!(table.contains(want), "table misses '{want}': {table}");
    }
    assert!(table.contains("stamp"), "{table}");
}

/// `studio restore` brings an archived studio back as a finished
/// studio; a second restore of the same name refuses, and `--name`
/// renames it.
#[test]
fn restore_brings_a_studio_back_finished() {
    let (scratch, env) = scratch();
    archived_studio(&scratch, &env, "repo-shop", "acme-shop");
    let archived_name = store::archived_studios(&scratch.dir).unwrap()[0].0.clone();

    // A dry run changes nothing.
    let dry = ok(
        &env,
        &format!("studio restore {archived_name} --dry-run --as user"),
    );
    assert!(dry.contains("would restore"), "{dry}");
    assert!(!store::studio_dir(&scratch.dir, "acme-shop").exists());

    let out = ok(&env, &format!("studio restore {archived_name} --as user"));
    assert!(out.contains("restored"), "{out}");
    let dir = store::studio_dir(&scratch.dir, "acme-shop");
    assert!(dir.join("events.jsonl").is_file());

    // It comes back finished and listed as its own studio.
    let list = ok(&env, "studio list");
    assert!(list.contains("acme-shop"), "{list}");
    assert!(list.contains("finished"), "{list}");
    let state = store::load_state(&dir).unwrap();
    assert!(state.finished.is_some());

    // The name is taken now: a second restore refuses, `--name` works.
    let err = refused(&env, &format!("studio restore {archived_name} --as user"));
    assert!(err.contains("already exists"), "{err}");
    let err = refused(
        &env,
        &format!("studio restore {archived_name} --name acme-shop --as user"),
    );
    assert!(err.contains("already exists"), "{err}");
    let out = ok(
        &env,
        &format!("studio restore {archived_name} --name acme-shop-copy --as user"),
    );
    assert!(out.contains("acme-shop-copy"), "{out}");
    assert!(store::studio_dir(&scratch.dir, "acme-shop-copy").is_dir());
}

/// Only the user restores; a working role is refused.
#[test]
fn restore_is_the_users() {
    let (_scratch, env) = scratch();
    let err = refused(&env, "studio restore whatever --as producer");
    assert!(err.contains("the user"), "{err}");
}

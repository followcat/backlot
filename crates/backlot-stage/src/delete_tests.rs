//! Unit & CLI tests for `backlot studio delete`, archive storage,
//! cross-studio `records`, `cast --suggest`, and re-creating a deleted studio id.

use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::Env;
use crate::cli_testkit::*;
use crate::store;

fn git_repo(path: &Path) {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "-b", "main"]);
    fs::write(path.join("README.md"), "base\n").unwrap();
    git(path, &["add", "."]);
    git(path, &["commit", "-m", "base"]);
}

fn setup_delivered_studio(
    scratch: &Scratch,
    env: &Env,
    repo: &Path,
    studio_name: &str,
    merge_to_main: bool,
) -> PathBuf {
    git_repo(repo);
    let cast = fixture(scratch, &format!("cast-{studio_name}.toml"), CAST_TOML);
    let req = fixture(scratch, &format!("req-{studio_name}.md"), "# req\n");
    let acc = fixture(scratch, &format!("acc-{studio_name}.md"), "# acc\n");
    let rep = fixture(scratch, &format!("rep-{studio_name}.md"), "# report\n");
    ok(
        env,
        &format!(
            "studio new --studio {studio_name} --title {studio_name} --repo {} --cast {cast}",
            repo.display()
        ),
    );
    ok(
        env,
        &format!("req revise --studio {studio_name} --file {req} --summary s --as producer"),
    );
    ok(
        env,
        &format!(
            "task new --studio {studio_name} --title T1 --kind feature --acceptance-file {acc} --as director"
        ),
    );
    ok(
        env,
        &format!(
            "cast --studio {studio_name} --task t1 --actor actor-a --reason fit --as director"
        ),
    );
    let work = scratch
        .dir
        .join("studios")
        .join(studio_name)
        .join("work/t1");
    fs::create_dir_all(work.parent().unwrap()).unwrap();
    let branch = format!("backlot/{studio_name}/t1");
    git(
        repo,
        &["worktree", "add", "-b", &branch, work.to_str().unwrap()],
    );
    let base_sha = git(&work, &["rev-parse", "HEAD"]);
    fs::write(work.join("feature.txt"), "work\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-m", "deliver"]);
    let tip = git(&work, &["rev-parse", "HEAD"]);
    ok(
        env,
        &format!(
            "deliver --studio {studio_name} --task t1 --commits {base_sha}..{tip} --report {rep} --as actor-t1-c1"
        ),
    );
    ok(
        env,
        &format!(
            "review --studio {studio_name} --task t1 --delivery 1 --verdict accepted --conclusion ok --as director"
        ),
    );
    if merge_to_main {
        git(repo, &["merge", "--ff-only", &branch]);
    }
    work
}

#[test]
fn studio_delete_archives_keeps_records_and_allows_recreate() {
    let (scratch, env) = scratch();
    let repo = scratch.files.join("repo-shop");
    let work = setup_delivered_studio(&scratch, &env, &repo, "acme-shop", true);

    // Non-user role is refused.
    let err = refused(&env, "studio delete acme-shop --as producer");
    assert!(err.contains("the user"), "{err}");

    // Dry-run changes nothing.
    let dry = ok(&env, "studio delete acme-shop --dry-run --json --as user");
    let dry_v: serde_json::Value = serde_json::from_str(&dry).unwrap();
    assert_eq!(dry_v["dry_run"], true);
    assert_eq!(dry_v["archived"], false);
    assert_eq!(dry_v["finished"], true);
    assert!(work.exists(), "dry-run must not remove worktree");
    assert!(store::studio_dir(&scratch.dir, "acme-shop").exists());

    // Real delete finishes, prunes clean worktree & merged branch, appends
    // studio.deleted, and archives to archive/acme-shop-<stamp>.
    let out = ok(&env, "studio delete acme-shop --json --as user");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["archived"], true, "{out}");
    assert_eq!(v["finished"], true, "{out}");
    assert!(!v["removed_worktrees"].as_array().unwrap().is_empty());
    assert!(!v["deleted_branches"].as_array().unwrap().is_empty());
    assert!(!work.exists(), "worktree should be removed");
    assert!(
        !store::studio_dir(&scratch.dir, "acme-shop").exists(),
        "active studio dir should be gone"
    );

    let archived = store::archived_studios(&scratch.dir).unwrap();
    assert_eq!(archived.len(), 1);
    assert!(archived[0].0.starts_with("acme-shop-"));
    let (events, _) = store::load_events(&archived[0].1).unwrap();
    assert!(events.iter().any(|e| matches!(
        &e.kind,
        backlot_model::event::EventKind::StudioDeleted { archive } if archive == &format!("archive/{}", archived[0].0)
    )));

    // `studio list` excludes the deleted studio.
    let list_json = ok(&env, "studio list --json");
    assert!(!list_json.contains("acme-shop"), "{list_json}");

    // `records` still includes the archived studio's actor delivery!
    let recs = ok(&env, "records --json");
    let recs_v: serde_json::Value = serde_json::from_str(&recs).unwrap();
    let row = &recs_v.as_array().unwrap()[0];
    assert_eq!(row["tasks"], 1);
    assert_eq!(row["studios"][0], archived[0].0);

    // Re-creating the same id gets a fresh ledger and contributes a second
    // distinct StudioLog entry to `records` and `cast --suggest`.
    let repo2 = scratch.files.join("repo-shop-2");
    setup_delivered_studio(&scratch, &env, &repo2, "acme-shop", true);
    let recs2: serde_json::Value = serde_json::from_str(&ok(&env, "records --json")).unwrap();
    assert_eq!(recs2[0]["tasks"], 2);
    assert_eq!(recs2[0]["studios"].as_array().unwrap().len(), 2);

    let acc2 = fixture(&scratch, "acc2.md", "# acc2\n");
    ok(
        &env,
        &format!(
            "task new --studio acme-shop --title T2 --kind feature --acceptance-file {acc2} --as director"
        ),
    );
    let suggest: serde_json::Value = serde_json::from_str(&ok(
        &env,
        "cast --studio acme-shop --task t2 --suggest --json",
    ))
    .unwrap();
    assert_eq!(suggest[0]["exact_kind"]["tasks"], 2);
}

#[test]
fn studio_delete_keeps_dirty_worktree_and_unmerged_branch_until_force() {
    let (scratch, env) = scratch();
    let repo = scratch.files.join("repo-dirty");
    let work = setup_delivered_studio(&scratch, &env, &repo, "acme-shop", false);
    fs::write(work.join("dirty.txt"), "uncommitted\n").unwrap();

    // Without --force: exits non-zero, returns JSON with kept items, and does NOT archive.
    let err = refused(&env, "studio delete acme-shop --json --as user");
    let v: serde_json::Value = serde_json::from_str(&err).unwrap();
    assert_eq!(v["archived"], false, "{err}");
    assert!(!v["kept"].as_array().unwrap().is_empty(), "{err}");
    assert!(
        store::studio_dir(&scratch.dir, "acme-shop").exists(),
        "must not archive when items are kept"
    );
    assert!(store::archived_studios(&scratch.dir).unwrap().is_empty());

    // With --force: removes dirty worktree + unmerged branch and archives.
    let out = ok(&env, "studio delete acme-shop --force --json --as user");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["archived"], true, "{out}");
    assert!(v["kept"].as_array().unwrap().is_empty(), "{out}");
    assert!(!work.exists());
    assert!(!store::studio_dir(&scratch.dir, "acme-shop").exists());
    assert_eq!(store::archived_studios(&scratch.dir).unwrap().len(), 1);
}

#[test]
fn studio_delete_refuses_open_task_without_touching_worktree_or_archiving() {
    let (scratch, env) = scratch();
    let repo = scratch.files.join("repo-open");
    git_repo(&repo);
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    let req = fixture(&scratch, "req.md", "# req\n");
    let acc = fixture(&scratch, "acc.md", "# acc\n");
    ok(
        &env,
        &format!(
            "studio new --studio acme-shop --title acme --repo {} --cast {cast}",
            repo.display()
        ),
    );
    ok(
        &env,
        &format!("req revise --studio acme-shop --file {req} --summary s --as producer"),
    );
    ok(
        &env,
        &format!(
            "task new --studio acme-shop --title T1 --kind feature --acceptance-file {acc} --as director"
        ),
    );
    let err = refused(&env, "studio delete acme-shop --as user");
    assert!(err.contains("t1"), "{err}");
    assert!(store::studio_dir(&scratch.dir, "acme-shop").exists());
    assert!(store::archived_studios(&scratch.dir).unwrap().is_empty());
}

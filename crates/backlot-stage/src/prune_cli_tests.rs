//! The `studio prune` CLI side: a live studio exits 2, a finished one
//! is really reaped by real git, the ledger survives, and the parser
//! demands exactly one scope.

use std::fs;

use crate::cli_testkit::{CAST_TOML, fixture, git, ok, refused, scratch, usage};
use crate::store;

/// The CLI refuses a live studio with a usage error (exit 2) and
/// `--all-finished` quietly skips one; a finished studio really has its
/// worktrees and branches reaped while the ledger and records survive.
#[test]
fn the_cli_refuses_live_studios_and_prunes_finished_ones() {
    let (scratch, env) = scratch();
    let repo = scratch.files.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("f"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            repo.display()
        ),
    );

    // The studio directory and its two worktrees, as the stage makes
    // them.
    let studio_dir = store::studio_dir(&scratch.dir, "acme-shop");
    let work = studio_dir.join("work");
    fs::create_dir_all(&work).unwrap();
    let integration = work.join("integration");
    let t1 = work.join("t1");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "backlot/acme-shop/integration",
            integration.to_str().unwrap(),
        ],
    );
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "backlot/acme-shop/t1",
            t1.to_str().unwrap(),
        ],
    );
    fs::write(t1.join("dirty"), "work in progress\n").unwrap();
    // The history prune must never delete.
    fs::create_dir_all(studio_dir.join("reports")).unwrap();
    fs::write(studio_dir.join("reports/r.md"), "keep\n").unwrap();
    fs::create_dir_all(studio_dir.join("messages")).unwrap();
    fs::write(studio_dir.join("messages/m.md"), "keep\n").unwrap();

    // Live: exit 2, and nothing moves.
    let m = usage(&env, "studio prune acme-shop --dry-run");
    assert!(m.contains("not finished"), "{m}");
    assert!(integration.exists() && t1.exists());
    assert_eq!(
        ok(&env, "studio prune --all-finished"),
        "no finished studios to prune\n"
    );

    ok(&env, "studio finish --reason done --as user");

    // Dry run plans and changes nothing.
    let dry = ok(&env, "studio prune acme-shop --dry-run");
    for want in [
        "dry run: studio acme-shop (finished): prune",
        "would remove worktree work/integration",
        "kept worktree work/t1 — uncommitted changes",
        "would delete branch backlot/acme-shop/integration",
        "kept branch backlot/acme-shop/t1 — worktree kept",
    ] {
        assert!(dry.contains(want), "dry run misses {want}:\n{dry}");
    }
    assert!(integration.exists() && t1.exists());
    assert!(
        git(
            &repo,
            &["branch", "--list", "backlot/acme-shop/integration"]
        )
        .contains("integration")
    );

    // The safe pass: the clean tree and its merged branch go, the
    // dirty tree and its branch stay.
    let out = ok(&env, "studio prune acme-shop");
    assert!(out.contains("removed worktree work/integration"), "{out}");
    assert!(
        out.contains("kept worktree work/t1 — uncommitted changes"),
        "{out}"
    );
    assert!(
        out.contains("deleted branch backlot/acme-shop/integration"),
        "{out}"
    );
    assert!(out.contains("kept branch backlot/acme-shop/t1 —"), "{out}");
    assert!(!integration.exists());
    assert!(t1.exists());
    assert!(
        git(
            &repo,
            &["branch", "--list", "backlot/acme-shop/integration"]
        )
        .is_empty()
    );
    assert!(git(&repo, &["branch", "--list", "backlot/acme-shop/t1"]).contains("t1"));

    // --force takes the rest.
    let out = ok(&env, "studio prune acme-shop --force");
    assert!(out.contains("removed worktree work/t1"), "{out}");
    assert!(out.contains("deleted branch backlot/acme-shop/t1"), "{out}");
    assert!(!t1.exists());
    assert!(git(&repo, &["branch", "--list", "backlot/acme-shop/*"]).is_empty());

    // The ledger and the records are untouched.
    assert!(studio_dir.join("events.jsonl").is_file());
    assert_eq!(
        fs::read_to_string(studio_dir.join("reports/r.md")).unwrap(),
        "keep\n"
    );
    assert_eq!(
        fs::read_to_string(studio_dir.join("messages/m.md")).unwrap(),
        "keep\n"
    );

    // A second prune has nothing left.
    assert!(ok(&env, "studio prune acme-shop").contains("nothing to prune"));
}

/// The parser demands exactly one scope and knows its flags.
#[test]
fn the_parser_requires_a_scope() {
    let (_scratch, env) = scratch();
    assert!(usage(&env, "studio prune").contains("missing studio"));
    assert!(
        usage(&env, "studio prune acme-shop --all-finished").contains("not both"),
        "a studio and --all-finished are exclusive"
    );
    assert!(
        refused(&env, "studio prune nosuch --dry-run").contains("no studio 'nosuch'"),
        "an absent studio is refused"
    );
}

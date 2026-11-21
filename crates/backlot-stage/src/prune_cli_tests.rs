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

/// A workspace-bound studio: the bound path is a plain parent
/// directory and the git trees are its member repositories. Prune
/// drains each member's `work/integration/<member>` worktree and
/// `backlot/<studio>/integration` branch — the shapes the stage
/// really creates — and does not refuse the studio.
#[test]
fn a_workspace_bound_studio_prunes_every_member() {
    let (scratch, env) = scratch();
    // A parent directory holding two member repositories, each with
    // base history on `main`.
    let ws = scratch.files.join("workspace");
    fs::create_dir_all(&ws).unwrap();
    let mut members = Vec::new();
    for name in ["svc-api", "svc-web"] {
        let repo = ws.join(name);
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-b", "main"]);
        fs::write(repo.join("f"), "base\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "base"]);
        members.push(repo);
    }
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            ws.display()
        ),
    );

    // One integration worktree per member under `work/integration/`,
    // all on the studio's single integration branch name (D10).
    let studio_dir = store::studio_dir(&scratch.dir, "acme-shop");
    let work = studio_dir.join("work").join("integration");
    fs::create_dir_all(&work).unwrap();
    for member in &members {
        let leaf = member.file_name().unwrap().to_str().unwrap().to_string();
        git(
            member,
            &[
                "worktree",
                "add",
                "-b",
                "backlot/acme-shop/integration",
                work.join(&leaf).to_str().unwrap(),
            ],
        );
    }
    fs::create_dir_all(studio_dir.join("reports")).unwrap();
    fs::write(studio_dir.join("reports/r.md"), "keep\n").unwrap();

    ok(&env, "studio finish --reason done --as user");

    // The dry run plans per member and refuses nothing.
    let dry = ok(&env, "studio prune acme-shop --dry-run");
    for want in [
        "dry run: studio acme-shop (finished): prune",
        "would remove worktree work/integration/svc-api",
        "would remove worktree work/integration/svc-web",
        "would delete branch svc-api: backlot/acme-shop/integration",
        "would delete branch svc-web: backlot/acme-shop/integration",
    ] {
        assert!(dry.contains(want), "dry run misses {want}:\n{dry}");
    }
    assert!(
        work.join("svc-api").exists() && work.join("svc-web").exists(),
        "the dry run moved something"
    );

    // The real pass: both member worktrees and both branches go.
    let out = ok(&env, "studio prune acme-shop");
    assert!(
        out.contains("removed worktree work/integration/svc-api"),
        "{out}"
    );
    assert!(
        out.contains("removed worktree work/integration/svc-web"),
        "{out}"
    );
    for leaf in ["svc-api", "svc-web"] {
        assert!(
            out.contains(&format!(
                "deleted branch {leaf}: backlot/acme-shop/integration"
            )),
            "no deletion for {leaf}: {out}"
        );
    }
    for member in &members {
        assert!(
            git(member, &["worktree", "list"]).lines().count() == 1,
            "{} still has a linked worktree",
            member.display()
        );
        assert!(
            git(member, &["branch", "--list", "backlot/acme-shop/*"]).is_empty(),
            "{} kept a studio branch",
            member.display()
        );
    }
    // The ledger and the records survive, and a second pass is idle.
    assert!(studio_dir.join("events.jsonl").is_file());
    assert_eq!(
        fs::read_to_string(studio_dir.join("reports/r.md")).unwrap(),
        "keep\n"
    );
    assert!(ok(&env, "studio prune acme-shop").contains("nothing to prune"));
}

/// The recorded member list is authoritative, not the directory: a
/// repository dropped into the workspace after the binding is never
/// probed or pruned, and its own branches stay.
#[test]
fn a_repository_added_after_the_binding_is_left_alone() {
    let (scratch, env) = scratch();
    let ws = scratch.files.join("workspace");
    fs::create_dir_all(&ws).unwrap();
    let api = ws.join("svc-api");
    fs::create_dir_all(&api).unwrap();
    git(&api, &["init", "-b", "main"]);
    fs::write(api.join("f"), "base\n").unwrap();
    git(&api, &["add", "."]);
    git(&api, &["commit", "-m", "base"]);
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            ws.display()
        ),
    );

    // A repository the ledger never recorded, carrying a branch that
    // merely looks like the studio's.
    let late = ws.join("svc-late");
    fs::create_dir_all(&late).unwrap();
    git(&late, &["init", "-b", "main"]);
    fs::write(late.join("f"), "base\n").unwrap();
    git(&late, &["add", "."]);
    git(&late, &["commit", "-m", "base"]);
    git(&late, &["branch", "backlot/acme-shop/integration"]);

    let studio_dir = store::studio_dir(&scratch.dir, "acme-shop");
    let work = studio_dir.join("work").join("integration");
    fs::create_dir_all(&work).unwrap();
    git(
        &api,
        &[
            "worktree",
            "add",
            "-b",
            "backlot/acme-shop/integration",
            work.join("svc-api").to_str().unwrap(),
        ],
    );

    ok(&env, "studio finish --reason done --as user");
    let out = ok(&env, "studio prune acme-shop --force");
    assert!(
        out.contains("removed worktree work/integration/svc-api"),
        "{out}"
    );
    assert!(
        !out.contains("svc-late"),
        "the unrecorded repository was reported: {out}"
    );
    assert!(
        git(&late, &["branch", "--list", "backlot/acme-shop/*"]).contains("integration"),
        "an unrecorded repository's branch was deleted"
    );
}

/// A recorded member that is gone or broken is kept with its reason
/// and the other members still prune — exit stays ok, so
/// `--all-finished` walks past it.
#[test]
fn a_broken_member_is_kept_and_the_others_still_prune() {
    let (scratch, env) = scratch();
    let ws = scratch.files.join("workspace");
    fs::create_dir_all(&ws).unwrap();
    let mut repos = Vec::new();
    for name in ["svc-api", "svc-web"] {
        let repo = ws.join(name);
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-b", "main"]);
        fs::write(repo.join("f"), "base\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "base"]);
        repos.push(repo);
    }
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            ws.display()
        ),
    );
    let studio_dir = store::studio_dir(&scratch.dir, "acme-shop");
    let work = studio_dir.join("work").join("integration");
    fs::create_dir_all(&work).unwrap();
    for repo in &repos {
        let leaf = repo.file_name().unwrap().to_str().unwrap().to_string();
        git(
            repo,
            &[
                "worktree",
                "add",
                "-b",
                "backlot/acme-shop/integration",
                work.join(&leaf).to_str().unwrap(),
            ],
        );
    }
    ok(&env, "studio finish --reason done --as user");

    // `svc-web` is no longer a git repository — moved away, or its
    // `.git` destroyed. The other member must still prune.
    fs::remove_dir_all(repos[1].join(".git")).unwrap();

    let out = ok(&env, "studio prune acme-shop");
    assert!(
        out.contains("kept svc-web — not a git work tree"),
        "the broken member went unreported: {out}"
    );
    assert!(
        out.contains("removed worktree work/integration/svc-api"),
        "{out}"
    );
    assert!(
        out.contains("deleted branch svc-api: backlot/acme-shop/integration"),
        "{out}"
    );
    assert!(
        !git(&repos[0], &["branch", "--list", "backlot/acme-shop/*"]).contains("integration"),
        "the good member kept its branch"
    );
}

/// A recorded member that passes its probe but then refuses a git
/// step is kept as a whole and the others still prune: one member's
/// trouble never strands the studio.
#[test]
fn a_member_failing_midway_is_kept_while_the_others_prune() {
    let (scratch, env) = scratch();
    let ws = scratch.files.join("workspace");
    fs::create_dir_all(&ws).unwrap();
    let mut repos = Vec::new();
    for name in ["svc-api", "svc-web"] {
        let repo = ws.join(name);
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-b", "main"]);
        fs::write(repo.join("f"), "base\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "base"]);
        repos.push(repo);
    }
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            ws.display()
        ),
    );
    let studio_dir = store::studio_dir(&scratch.dir, "acme-shop");
    let work = studio_dir.join("work").join("integration");
    fs::create_dir_all(&work).unwrap();
    for repo in &repos {
        let leaf = repo.file_name().unwrap().to_str().unwrap().to_string();
        git(
            repo,
            &[
                "worktree",
                "add",
                "-b",
                "backlot/acme-shop/integration",
                work.join(&leaf).to_str().unwrap(),
            ],
        );
    }
    ok(&env, "studio finish --reason done --as user");

    // `svc-web` still answers the probe but its worktree listing is
    // broken — the shape a damaged git dir takes.
    let broken = repos[1].join(".git");
    let stash = repos[1].join(".git-stash");
    fs::rename(&broken, &stash).unwrap();
    fs::write(&broken, "gitdir: /nonexistent/nowhere\n").unwrap();

    let out = ok(&env, "studio prune acme-shop");
    assert!(
        out.contains("kept svc-web —"),
        "the failing member went unreported: {out}"
    );
    assert!(
        out.contains("removed worktree work/integration/svc-api"),
        "{out}"
    );
    assert!(
        !git(&repos[0], &["branch", "--list", "backlot/acme-shop/*"]).contains("integration"),
        "the good member kept its branch"
    );
}

/// A no-git studio has no worktrees and no studio branches: prune
/// reports it idle instead of refusing it as "not a git work tree".
#[test]
fn a_no_git_studio_prunes_without_git_steps() {
    let (scratch, env) = scratch();
    let dir = scratch.files.join("plain");
    fs::create_dir_all(&dir).unwrap();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast} --no-git",
            dir.display()
        ),
    );
    let studio_dir = store::studio_dir(&scratch.dir, "acme-shop");
    fs::create_dir_all(studio_dir.join("reports")).unwrap();
    fs::write(studio_dir.join("reports/r.md"), "keep\n").unwrap();

    ok(&env, "studio finish --reason done --as user");

    assert_eq!(
        ok(&env, "studio prune acme-shop --dry-run"),
        "dry run: studio acme-shop (finished): prune\n  nothing to prune\n"
    );
    assert!(ok(&env, "studio prune acme-shop").contains("nothing to prune"));
    // Its state records are untouched, its bound path never written.
    assert!(studio_dir.join("events.jsonl").is_file());
    assert_eq!(
        fs::read_to_string(studio_dir.join("reports/r.md")).unwrap(),
        "keep\n"
    );
    assert!(dir.is_dir(), "prune touched the bound directory");
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

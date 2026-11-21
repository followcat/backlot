//! `studio prune`: the safe/force/dry-run planning over an injected
//! git, plus missing trees and foreign refs. The CLI end-to-end test
//! lives in `prune_cli_tests`.

use std::fs;
use std::path::Path;

use crate::prune::{Step, prune_studio};
use crate::test_support::TestDir;

/// A recording fake git for the planning tests.
struct Fake {
    calls: Vec<String>,
    list: String,
    dirty: Vec<String>,
    branches: Vec<String>,
    merged: Vec<String>,
    fail_remove: bool,
}

impl Fake {
    fn out(code: i32, stdout: &str, stderr: &str) -> crate::workareas::GitOut {
        crate::workareas::GitOut {
            code,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    fn run(&mut self, argv: &[&str], cwd: &str) -> crate::workareas::GitOut {
        let joined = argv.join(" ");
        self.calls.push(format!("[{cwd}] {joined}"));
        if joined.ends_with("worktree list --porcelain") {
            return Self::out(0, &self.list, "");
        }
        if joined == "status --porcelain" {
            let dirty = self.dirty.iter().any(|p| cwd == p);
            return Self::out(0, if dirty { " M f\n" } else { "" }, "");
        }
        if joined.contains("worktree remove") {
            return if self.fail_remove {
                Self::out(1, "", "fatal: could not remove\n")
            } else {
                Self::out(0, "", "")
            };
        }
        if joined.ends_with("worktree prune") {
            return Self::out(0, "", "");
        }
        if joined.contains("branch --list") {
            let mut s = self.branches.join("\n");
            s.push('\n');
            return Self::out(0, &s, "");
        }
        if let Some(rest) = joined.split("merge-base --is-ancestor ").nth(1) {
            let branch = rest.split_whitespace().next().unwrap_or_default();
            let code = i32::from(!self.merged.iter().any(|b| b == branch));
            return Self::out(code, "", "");
        }
        if let Some(rest) = joined.split("branch -d ").nth(1) {
            return self.delete(rest, false);
        }
        if let Some(rest) = joined.split("branch -D ").nth(1) {
            return self.delete(rest, true);
        }
        Self::out(0, "", "")
    }

    fn delete(&self, branch: &str, force: bool) -> crate::workareas::GitOut {
        if force || self.merged.iter().any(|b| b == branch) {
            Self::out(0, "", "")
        } else {
            Self::out(
                1,
                "",
                &format!("error: the branch '{branch}' is not fully merged\n"),
            )
        }
    }
}

fn git_closure(fake: &mut Fake) -> impl FnMut(&[&str], &str) -> crate::workareas::GitOut + '_ {
    move |argv, cwd| fake.run(argv, cwd)
}

fn tree(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn listing(rows: &[(String, String)]) -> String {
    let mut s = String::new();
    for (path, branch) in rows {
        s.push_str(&format!(
            "worktree {path}\nHEAD 0123\nbranch refs/heads/{branch}\n\n"
        ));
    }
    s
}

/// A clean worktree goes, a dirty one stays without `--force`, and a
/// branch goes only when `git branch -d` accepts it. `--dry-run` reads
/// and prints, changing nothing.
#[test]
fn safe_and_dry_run_plans() {
    let dir = TestDir::new("backlot-prune-safe");
    let repo = dir.join("repo");
    let work = dir.join("work");
    fs::create_dir_all(&work).unwrap();
    let integration = work.join("integration");
    let t1 = work.join("t1");
    fs::create_dir_all(&integration).unwrap();
    fs::create_dir_all(&t1).unwrap();
    let list = listing(&[
        (tree(&repo), "main".into()),
        (tree(&integration), "backlot/s/integration".into()),
        (tree(&t1), "backlot/s/t1".into()),
    ]);
    let mut fake = Fake {
        calls: Vec::new(),
        list,
        dirty: vec![tree(&t1)],
        branches: vec![
            "backlot/s/integration".into(),
            "backlot/s/t1".into(),
            "other/keep".into(),
        ],
        merged: vec!["backlot/s/integration".into()],
        fail_remove: false,
    };
    let repo_s = tree(&repo);

    // Dry run: the plan, with no mutating git call.
    let steps = {
        let mut git = git_closure(&mut fake);
        prune_studio(&mut git, "s", &dir, &repo_s, None, true, false).unwrap()
    };
    assert_eq!(
        steps,
        vec![
            Step::RemovedWorktree("work/integration".into()),
            Step::KeptWorktree("work/t1".into(), "uncommitted changes".into()),
            Step::DeletedBranch("backlot/s/integration".into()),
            Step::KeptBranch("backlot/s/t1".into(), "worktree kept".into()),
        ]
    );
    for banned in [
        "worktree remove",
        "worktree prune",
        "branch -d",
        "branch -D",
    ] {
        assert!(
            !fake.calls.iter().any(|c| c.contains(banned)),
            "dry run ran {banned}: {:?}",
            fake.calls
        );
    }

    // The real safe pass.
    fake.calls.clear();
    let steps = {
        let mut git = git_closure(&mut fake);
        prune_studio(&mut git, "s", &dir, &repo_s, None, false, false).unwrap()
    };
    assert_eq!(
        steps,
        vec![
            Step::RemovedWorktree("work/integration".into()),
            Step::KeptWorktree("work/t1".into(), "uncommitted changes".into()),
            Step::DeletedBranch("backlot/s/integration".into()),
            Step::KeptBranch(
                "backlot/s/t1".into(),
                "error: the branch 'backlot/s/t1' is not fully merged".into()
            ),
        ]
    );
    let removes: Vec<&String> = fake
        .calls
        .iter()
        .filter(|c| c.contains("worktree remove"))
        .collect();
    assert_eq!(removes.len(), 1, "{removes:?}");
    assert!(!removes[0].contains("--force"), "{removes:?}");
    assert!(
        fake.calls
            .iter()
            .any(|c| c.contains("branch -d backlot/s/integration")),
        "{:?}",
        fake.calls
    );
    assert!(
        fake.calls
            .iter()
            .any(|c| c.contains("branch -d backlot/s/t1")),
        "{:?}",
        fake.calls
    );

    // --force takes the dirty tree and the unmerged branch too.
    fake.calls.clear();
    let steps = {
        let mut git = git_closure(&mut fake);
        prune_studio(&mut git, "s", &dir, &repo_s, None, false, true).unwrap()
    };
    assert_eq!(
        steps,
        vec![
            Step::RemovedWorktree("work/integration".into()),
            Step::RemovedWorktree("work/t1".into()),
            Step::DeletedBranch("backlot/s/integration".into()),
            Step::DeletedBranch("backlot/s/t1".into()),
        ]
    );
    assert!(
        fake.calls
            .iter()
            .any(|c| c.contains("worktree remove --force")),
        "{:?}",
        fake.calls
    );
    assert!(
        fake.calls
            .iter()
            .any(|c| c.contains("branch -D backlot/s/t1")),
        "{:?}",
        fake.calls
    );
}

/// A registered worktree whose folder vanished is pruned once, and a
/// worktree or branch outside this studio's scope is never touched.
#[test]
fn missing_trees_are_pruned_and_foreign_refs_left_alone() {
    let dir = TestDir::new("backlot-prune-missing");
    let repo = dir.join("repo");
    let work = dir.join("work");
    fs::create_dir_all(&work).unwrap();
    let gone = work.join("t1");
    let list = listing(&[
        (tree(&dir.join("other")), "feature/x".into()),
        (tree(&gone), "backlot/s/t1".into()),
    ]);
    let mut fake = Fake {
        calls: Vec::new(),
        list,
        dirty: Vec::new(),
        branches: vec![
            "backlot/s/t1".into(),
            "backlot/s2/t1".into(),
            "feature/x".into(),
        ],
        merged: Vec::new(),
        fail_remove: false,
    };
    let repo_s = tree(&repo);
    let steps = {
        let mut git = git_closure(&mut fake);
        prune_studio(&mut git, "s", &dir, &repo_s, None, false, false).unwrap()
    };
    assert_eq!(
        steps,
        vec![
            Step::PrunedWorktree("work/t1".into()),
            Step::KeptBranch(
                "backlot/s/t1".into(),
                "error: the branch 'backlot/s/t1' is not fully merged".into()
            ),
        ]
    );
    assert_eq!(
        fake.calls
            .iter()
            .filter(|c| c.ends_with("worktree prune"))
            .count(),
        1,
        "{:?}",
        fake.calls
    );
    let deleted: Vec<&String> = fake
        .calls
        .iter()
        .filter(|c| c.contains("branch -d") || c.contains("branch -D"))
        .collect();
    assert_eq!(deleted.len(), 1, "{:?}", fake.calls);
    assert!(
        deleted[0].contains("branch -d backlot/s/t1"),
        "{:?}",
        fake.calls
    );
    assert!(
        !deleted[0].contains("s2") && !deleted[0].contains("feature"),
        "a foreign branch was touched: {:?}",
        fake.calls
    );
}

/// macOS reaches its temp tree through `/var` → `/private/var`: git's
/// `worktree list` prints a resolved path while the state dir is
/// spelled with the symlink. Both an existing and a vanished worktree
/// must still read as `work/<name>` and stay in scope — whether git
/// prints the fully resolved path or one under the spelled parent.
#[cfg(unix)]
#[test]
fn a_symlinked_state_dir_still_matches_existing_and_missing_trees() {
    let root = TestDir::new("backlot-prune-symlink");
    let spelled = root.join("state-real");
    let work = spelled.join("work");
    fs::create_dir_all(&work).unwrap();
    fs::create_dir_all(work.join("t1")).unwrap();
    // Real git resolves the path it prints; the state dir stays a
    // symlink. Keep both spellings, so the two runs below differ only
    // in what `worktree list` reports.
    let real = spelled.canonicalize().unwrap();
    let existing = real.join("work").join("t1");
    let gone = real.join("work").join("t2");
    std::os::unix::fs::symlink(&real, root.join("state")).unwrap();
    let dir = root.join("state");
    let repo = root.join("repo");
    let expect = vec![
        Step::RemovedWorktree("work/t1".into()),
        Step::PrunedWorktree("work/t2".into()),
    ];
    let run = |list: String| {
        let mut fake = Fake {
            calls: Vec::new(),
            list,
            dirty: Vec::new(),
            branches: Vec::new(),
            merged: Vec::new(),
            fail_remove: false,
        };
        let mut git = git_closure(&mut fake);
        prune_studio(&mut git, "s", &dir, &tree(&repo), None, true, false).unwrap()
    };

    // What CI's macOS run saw: git prints the fully resolved paths.
    assert_eq!(
        run(listing(&[
            (tree(&existing), "backlot/s/t1".into()),
            (tree(&gone), "backlot/s/t2".into()),
        ])),
        expect
    );
    // And a path whose missing leaf has no canonicalize() of its own
    // — the parent is still spelled through the `/var` symlink.
    assert_eq!(
        run(listing(&[
            (
                tree(&spelled.join("work").join("t1")),
                "backlot/s/t1".into()
            ),
            (
                tree(&spelled.join("work").join("t2")),
                "backlot/s/t2".into()
            ),
        ])),
        expect
    );
}

/// A worktree remove that git refuses is reported as kept, never as a
/// success.
#[test]
fn a_refused_remove_is_kept_with_gits_line() {
    let dir = TestDir::new("backlot-prune-refuse");
    let work = dir.join("work");
    fs::create_dir_all(work.join("t1")).unwrap();
    let repo = dir.join("repo");
    let mut fake = Fake {
        calls: Vec::new(),
        list: listing(&[(tree(&work.join("t1")), "backlot/s/t1".into())]),
        dirty: Vec::new(),
        branches: Vec::new(),
        merged: Vec::new(),
        fail_remove: true,
    };
    let steps = {
        let mut git = git_closure(&mut fake);
        prune_studio(&mut git, "s", &dir, &tree(&repo), None, false, false).unwrap()
    };
    assert_eq!(
        steps,
        vec![Step::KeptWorktree(
            "work/t1".into(),
            "fatal: could not remove".into()
        )]
    );
}

// ---------- which repositories the binding names ----------

use crate::prune::{Plan, plan};
use backlot_model::state::StudioConfig;

/// A binding over `repo` with `members`, no git, nothing else set.
fn binding(repo: &str, members: &[&str], no_git: bool) -> StudioConfig {
    StudioConfig {
        title: "s".into(),
        repo: repo.into(),
        members: members.iter().map(|m| m.to_string()).collect(),
        no_git,
        mode: Default::default(),
        roles: backlot_model::event::RoleConfigs {
            producer: actor(),
            director: actor(),
            writer: actor(),
        },
        candidates: Vec::new(),
        rework_cap: 2,
        stall_limit_s: 600,
        nudge_after_s: 180,
        escalate_after_s: 600,
        check_env: Vec::new(),
        checks: Vec::new(),
        task_budget_s: None,
        limits: Default::default(),
    }
}

fn actor() -> backlot_model::event::ActorIdentity {
    backlot_model::event::ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "plain".into(),
        effort: None,
        permission_profile: "standard".into(),
    }
}

/// A workspace works its recorded members and nothing else: a
/// repository the parent holds but the ledger never recorded stays
/// untouched, even when its probe would succeed.
#[test]
fn a_workspace_works_exactly_its_recorded_members() {
    let mut probed = Vec::new();
    let mut git = |argv: &[&str], _cwd: &str| {
        let repo = argv[1].to_string();
        probed.push(repo.clone());
        Fake::out(0, "true\n", "")
    };
    let cfg = binding(
        "/home/user/ws",
        &["/home/user/ws/svc-api", "/home/user/ws/svc-web"],
        false,
    );
    let (plan, steps) = plan(&mut git, &cfg);
    assert_eq!(
        plan_repos(plan),
        vec!["/home/user/ws/svc-api", "/home/user/ws/svc-web"]
    );
    assert!(steps.is_empty(), "{steps:?}");
    assert_eq!(
        probed,
        vec!["/home/user/ws/svc-api", "/home/user/ws/svc-web"],
        "the parent was probed or scanned"
    );
}

fn plan_repos(plan: Plan) -> Vec<String> {
    match plan {
        Plan::Repos(repos) => repos,
        Plan::NotWorkTree => panic!("expected repos"),
    }
}

/// One recorded member that is not a work tree today is kept with its
/// reason and the others still run — a broken member never strands
/// the studio's other trees, nor aborts the pass.
#[test]
fn a_broken_member_is_kept_while_the_others_run() {
    let mut git = |argv: &[&str], _cwd: &str| match argv[1] {
        "/home/user/ws/gone" => Fake::out(128, "", "fatal: not a git repository\n"),
        _ => Fake::out(0, "true\n", ""),
    };
    let cfg = binding(
        "/home/user/ws",
        &["/home/user/ws/svc-api", "/home/user/ws/gone"],
        false,
    );
    let (plan, steps) = plan(&mut git, &cfg);
    assert_eq!(plan_repos(plan), vec!["/home/user/ws/svc-api"]);
    assert_eq!(
        steps,
        vec![Step::KeptRepo("gone".into(), "not a git work tree".into())]
    );
}

/// A no-git studio probes nothing: its state prunes and no git runs.
#[test]
fn a_no_git_studio_plans_no_repositories() {
    let mut probes = 0;
    let mut git = |_: &[&str], _: &str| {
        probes += 1;
        Fake::out(1, "", "fatal: not a git repository\n")
    };
    let cfg = binding("/home/user/plain", &[], true);
    let (plan, steps) = plan(&mut git, &cfg);
    assert!(plan_repos(plan).is_empty());
    assert!(steps.is_empty(), "{steps:?}");
    assert_eq!(probes, 0, "a no-git studio probed git");
}

/// A single repository keeps the one hard refusal when it is not a
/// work tree — the studio's whole layout lived there.
#[test]
fn a_single_repo_that_is_no_work_tree_refuses() {
    let mut git = |_: &[&str], _: &str| Fake::out(128, "", "fatal: not a git repository\n");
    let (plan, steps) = plan(&mut git, &binding("/home/user/repo", &[], false));
    assert!(matches!(plan, Plan::NotWorkTree));
    assert!(steps.is_empty(), "{steps:?}");
}

//! Work-area tests: a recording fake git proves the command order,
//! reuse and failure paths; one test drives the real git on a
//! temporary repository.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use backlot_model::event::ActorIdentity;

use crate::reconcile::{RoleKind, Start};
use crate::workareas::*;

static NEXT: AtomicU64 = AtomicU64::new(0);

fn tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "backlot-work-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A fake git that records every call and simulates the four verbs
/// the plan uses.
#[derive(Default)]
struct FakeGit {
    calls: Vec<String>,
    inside: bool,
    /// `(dir, branch)` rows `worktree list` reports.
    worktrees: Vec<(String, String)>,
    branches: BTreeSet<String>,
    /// Fail any call whose argv contains this text.
    fail: Option<String>,
}

impl FakeGit {
    fn run(&mut self, argv: &[&str], _cwd: &str) -> GitOut {
        let joined = argv.join(" ");
        self.calls.push(joined.clone());
        let out = |code: i32, stdout: String, stderr: String| GitOut {
            code,
            stdout,
            stderr,
        };
        if self.fail.as_ref().is_some_and(|f| joined.contains(f)) {
            return out(1, String::new(), "fatal: scripted failure\n".into());
        }
        let mut it = argv.iter();
        while let Some(a) = it.next() {
            match *a {
                "rev-parse" => match it.next() {
                    Some(&"--is-inside-work-tree") if self.inside => {
                        return out(0, "true\n".into(), String::new());
                    }
                    Some(&"--is-inside-work-tree") => {
                        return out(128, String::new(), "fatal: not a git repository\n".into());
                    }
                    Some(&"--verify") => {
                        let r = argv.last().unwrap();
                        let b = r.trim_start_matches("refs/heads/");
                        return out(
                            i32::from(!self.branches.contains(b)),
                            String::new(),
                            String::new(),
                        );
                    }
                    _ => {}
                },
                "worktree" => match it.next() {
                    Some(&"list") => {
                        let s: String = self
                            .worktrees
                            .iter()
                            .map(|(d, b)| {
                                format!("worktree {d}\nHEAD 0\nbranch refs/heads/{b}\n\n")
                            })
                            .collect();
                        return out(0, s, String::new());
                    }
                    Some(&"add") => {
                        let (dir, branch) = if argv[4] == "-b" {
                            self.branches.insert(argv[5].into());
                            (argv[6], argv[5])
                        } else {
                            (argv[4], argv[5])
                        };
                        // Real git creates the dir and `worktree list`
                        // reports its resolved path.
                        let _ = fs::create_dir_all(dir);
                        let real = Path::new(dir)
                            .canonicalize()
                            .unwrap_or_else(|_| PathBuf::from(dir));
                        self.worktrees
                            .push((real.to_string_lossy().into_owned(), branch.into()));
                        return out(0, String::new(), "Preparing worktree\n".into());
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        out(0, String::new(), String::new())
    }
}

fn start(kind: RoleKind, task: Option<&str>) -> Start {
    Start {
        role: "x".into(),
        kind,
        task: task.map(String::from),
        terminal: "t".into(),
        actor: ActorIdentity {
            runner: "fake".into(),
            runner_version: None,
            model: "m".into(),
            effort: Some("high".into()),
            permission_profile: "standard".into(),
        },
        repo: "/home/user/acme-shop".into(),
        resume: None,
    }
}

/// A fresh actor area runs the exact command order: repo probe,
/// list, branch probe, create — for integration first, then the
/// task off its tip.
#[test]
fn a_fresh_actor_area_branches_off_integration() {
    let dir = tmp();
    let mut fake = FakeGit {
        inside: true,
        ..FakeGit::default()
    };
    let mut git = |argv: &[&str], cwd: &str| fake.run(argv, cwd);
    let cwd = ensure(
        &mut git,
        "acme-shop",
        "/home/user/acme-shop",
        &dir,
        &Area::Task("t1".into()),
    )
    .unwrap();
    assert_eq!(cwd, dir.join("work").join("t1"));
    let repo = "/home/user/acme-shop";
    assert_eq!(
        fake.calls,
        vec![
            format!("-C {repo} rev-parse --is-inside-work-tree"),
            format!("-C {repo} worktree list --porcelain"),
            format!(
                "-C {repo} rev-parse --verify --quiet refs/heads/backlot/acme-shop/integration"
            ),
            format!(
                "-C {repo} worktree add -b backlot/acme-shop/integration {} HEAD",
                dir.join("work").join("integration").display()
            ),
            format!("-C {repo} worktree list --porcelain"),
            format!("-C {repo} rev-parse --verify --quiet refs/heads/backlot/acme-shop/t1"),
            format!(
                "-C {repo} worktree add -b backlot/acme-shop/t1 {} backlot/acme-shop/integration",
                dir.join("work").join("t1").display()
            ),
        ],
        "{:?}",
        fake.calls
    );
}

/// Existing worktrees and branches are reused — a recast runs only
/// the probes and attaches nothing, so work continues on top.
#[test]
fn an_existing_area_is_reused() {
    let dir = tmp();
    let integ = dir.join("work").join("integration");
    let t1 = dir.join("work").join("t1");
    // `worktree list` reports resolved paths of real dirs.
    fs::create_dir_all(&integ).unwrap();
    fs::create_dir_all(&t1).unwrap();
    let mut fake = FakeGit {
        inside: true,
        worktrees: vec![
            (
                integ.canonicalize().unwrap().display().to_string(),
                "backlot/acme-shop/integration".into(),
            ),
            (
                t1.canonicalize().unwrap().display().to_string(),
                "backlot/acme-shop/t1".into(),
            ),
        ],
        branches: BTreeSet::from([
            "backlot/acme-shop/integration".to_string(),
            "backlot/acme-shop/t1".to_string(),
        ]),
        ..FakeGit::default()
    };
    let mut git = |argv: &[&str], cwd: &str| fake.run(argv, cwd);
    let cwd = ensure(
        &mut git,
        "acme-shop",
        "/repo",
        &dir,
        &Area::Task("t1".into()),
    )
    .unwrap();
    assert_eq!(cwd, t1);
    assert!(
        !fake.calls.iter().any(|c| c.contains("add")),
        "reuse must not add: {:?}",
        fake.calls
    );
}

/// A worktree dir checked out elsewhere (a prune) reattaches the
/// existing branch instead of branching again.
#[test]
fn an_existing_branch_is_reattached() {
    let dir = tmp();
    let mut fake = FakeGit {
        inside: true,
        branches: BTreeSet::from(["backlot/acme-shop/integration".to_string()]),
        ..FakeGit::default()
    };
    let mut git = |argv: &[&str], cwd: &str| fake.run(argv, cwd);
    ensure(&mut git, "acme-shop", "/repo", &dir, &Area::Integration).unwrap();
    assert!(
        fake.calls.iter().any(|c| {
            c == &format!(
                "-C /repo worktree add {} backlot/acme-shop/integration",
                dir.join("work").join("integration").display()
            )
        }),
        "{:?}",
        fake.calls
    );
}

/// git's progress noise prints before its error — the attention
/// detail prefers the `fatal:`/`error:` line, else the first
/// non-blank line.
#[test]
fn err_line_prefers_the_fatal_line() {
    let noisy = GitOut {
        code: 128,
        stdout: String::new(),
        stderr: "Preparing worktree (checking out 'b/x')\nfatal: 'b/x' is already checked out\n"
            .into(),
    };
    assert_eq!(err_line(&noisy), "fatal: 'b/x' is already checked out");
    let plain = GitOut {
        code: 1,
        stdout: String::new(),
        stderr: "\n  \nsomething odd happened\nnext line\n".into(),
    };
    assert_eq!(err_line(&plain), "something odd happened");
    let none = GitOut {
        code: 2,
        stdout: String::new(),
        stderr: String::new(),
    };
    assert_eq!(err_line(&none), "exit 2");
}

/// A failing git reports its first stderr line — the retry issues
/// the same calls, so act can raise one attention and try again.
#[test]
fn a_git_failure_reports_stderr_and_retries() {
    let dir = tmp();
    let mut fake = FakeGit {
        inside: true,
        fail: Some("worktree add".into()),
        ..FakeGit::default()
    };
    let mut git = |argv: &[&str], cwd: &str| fake.run(argv, cwd);
    let first = ensure(&mut git, "s", "/repo", &dir, &Area::Integration);
    assert_eq!(first, Err(AreaFail::Git("fatal: scripted failure".into())));
    let second = ensure(&mut git, "s", "/repo", &dir, &Area::Integration);
    assert_eq!(second, Err(AreaFail::Git("fatal: scripted failure".into())));
}

/// A repo that is not a git work tree stops before any worktree
/// call — producer and writer never even probe.
#[test]
fn a_non_git_repo_starts_no_work_area() {
    let dir = tmp();
    let mut fake = FakeGit::default();
    let mut git = |argv: &[&str], cwd: &str| fake.run(argv, cwd);
    assert_eq!(
        ensure(&mut git, "s", "/repo", &dir, &Area::Integration),
        Err(AreaFail::NotGit)
    );
    assert_eq!(
        ensure(&mut git, "s", "/repo", &dir, &Area::Repo),
        Ok(PathBuf::from("/repo"))
    );
    assert_eq!(
        fake.calls,
        vec!["-C /repo rev-parse --is-inside-work-tree".to_string()],
        "Repo probes nothing and a dead repo stops at the probe"
    );
}

/// The area map: director to integration, actor to its task, the
/// rest to the repo; an actor without `task` parses its role key.
#[test]
fn area_and_role_dir_follow_the_role() {
    assert_eq!(area_of(&start(RoleKind::Producer, None)), Area::Repo);
    assert_eq!(area_of(&start(RoleKind::Writer, None)), Area::Repo);
    assert_eq!(area_of(&start(RoleKind::Director, None)), Area::Integration);
    assert_eq!(
        area_of(&start(RoleKind::Actor, Some("t2"))),
        Area::Task("t2".into())
    );
    let mut keyed = start(RoleKind::Actor, None);
    keyed.role = "actor-t7-c1".into();
    assert_eq!(area_of(&keyed), Area::Task("t7".into()));

    let dir = Path::new("/state/studios/acme-shop");
    assert_eq!(
        role_dir(dir, "/repo", "director"),
        PathBuf::from("/state/studios/acme-shop/work/integration")
    );
    assert_eq!(
        role_dir(dir, "/repo", "actor-t2-c1"),
        PathBuf::from("/state/studios/acme-shop/work/t2")
    );
    assert_eq!(role_dir(dir, "/repo", "producer"), PathBuf::from("/repo"));
}

fn git(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The one real-git test: a temporary repository gets real
/// worktrees and branches, a rerun reuses them, and a plain
/// directory is refused as `NotGit`.
#[test]
fn real_git_creates_real_worktrees() {
    if std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_err()
    {
        return; // no git on this host — the injected-runner tests still cover the plan
    }
    let root = tmp();
    let repo = root.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=alice",
            "-c",
            "user.email=alice@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "start",
        ],
    );
    let dir = root.join("state");
    fs::create_dir_all(&dir).unwrap();
    let repo_s = repo.to_string_lossy().into_owned();
    let mut git_run = real_git;
    let integ = ensure(&mut git_run, "acme-shop", &repo_s, &dir, &Area::Integration).unwrap();
    assert_eq!(integ, dir.join("work").join("integration"));
    assert_eq!(
        git(&integ, &["branch", "--show-current"]).trim(),
        "backlot/acme-shop/integration"
    );
    let t1 = ensure(
        &mut git_run,
        "acme-shop",
        &repo_s,
        &dir,
        &Area::Task("t1".into()),
    )
    .unwrap();
    assert_eq!(t1, dir.join("work").join("t1"));
    assert_eq!(
        git(&t1, &["branch", "--show-current"]).trim(),
        "backlot/acme-shop/t1"
    );
    // A rerun reuses both — the recast case.
    assert_eq!(
        ensure(
            &mut git_run,
            "acme-shop",
            &repo_s,
            &dir,
            &Area::Task("t1".into())
        ),
        Ok(t1.clone())
    );
    assert_eq!(
        ensure(&mut git_run, "acme-shop", &repo_s, &dir, &Area::Integration),
        Ok(integ)
    );
    // A plain directory is not a git work tree.
    let plain = root.join("plain");
    fs::create_dir_all(&plain).unwrap();
    assert_eq!(
        ensure(
            &mut git_run,
            "acme-shop",
            &plain.to_string_lossy(),
            &dir,
            &Area::Integration
        ),
        Err(AreaFail::NotGit)
    );
}

/// The same run with the state directory reached through a symlink
/// — macOS's `/var/folders` temp tree: `worktree list` prints the
/// resolved path, and reuse still sees it, so a second `ensure`
/// attaches nothing again.
#[test]
#[cfg(unix)]
fn real_git_through_a_symlinked_state_dir() {
    if std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let root = tmp();
    let repo = root.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=alice",
            "-c",
            "user.email=alice@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "start",
        ],
    );
    let real = root.join("state-real");
    fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink(&real, root.join("state")).unwrap();
    let dir = root.join("state");
    let repo_s = repo.to_string_lossy().into_owned();
    let mut git_run = real_git;
    let integ = ensure(&mut git_run, "acme-shop", &repo_s, &dir, &Area::Integration).unwrap();
    assert_eq!(integ, dir.join("work").join("integration"));
    let t1 = ensure(
        &mut git_run,
        "acme-shop",
        &repo_s,
        &dir,
        &Area::Task("t1".into()),
    )
    .unwrap();
    assert_eq!(t1, dir.join("work").join("t1"));
    assert!(real.join("work").join("integration").is_dir());
    assert!(real.join("work").join("t1").is_dir());
    // A rerun reuses both through the same symlinked path.
    assert_eq!(
        ensure(
            &mut git_run,
            "acme-shop",
            &repo_s,
            &dir,
            &Area::Task("t1".into())
        ),
        Ok(t1)
    );
    assert_eq!(
        ensure(&mut git_run, "acme-shop", &repo_s, &dir, &Area::Integration),
        Ok(integ)
    );
}

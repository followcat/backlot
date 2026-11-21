//! Work-area tests: a recording fake git proves the command order,
//! reuse and failure paths; one test drives the real git on a
//! temporary repository.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use backlot_model::event::ActorIdentity;

use crate::reconcile::{RoleKind, Start};
use crate::test_support::TestDir;
use crate::workareas::*;

fn tmp() -> TestDir {
    TestDir::new("backlot-work-test")
}

/// A fake git that records every call and simulates the verbs the
/// plan uses.
#[derive(Default)]
struct FakeGit {
    calls: Vec<String>,
    inside: bool,
    /// `(dir, branch)` rows `worktree list` reports.
    worktrees: Vec<(String, String)>,
    branches: BTreeSet<String>,
    /// Fail any call whose argv contains this text.
    fail: Option<String>,
    /// `member path → git-common-dir` answers for `rev-parse
    /// --git-common-dir`; an unconfigured member defaults to its
    /// own `.git`.
    common_dirs: BTreeMap<String, String>,
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
                    Some(&"--path-format=absolute") => {
                        let member = argv
                            .iter()
                            .position(|a| *a == "-C")
                            .and_then(|i| argv.get(i + 1))
                            .copied()
                            .unwrap_or("");
                        let dir = self.common_dirs.get(member).cloned().unwrap_or_else(|| {
                            Path::new(member)
                                .join(".git")
                                .to_string_lossy()
                                .into_owned()
                        });
                        return out(0, format!("{dir}\n"), String::new());
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
        members: Vec::new(),
        no_git: false,
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
        &[],
        false,
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
        &[],
        false,
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
    ensure(
        &mut git,
        "acme-shop",
        "/repo",
        &[],
        false,
        &dir,
        &Area::Integration,
    )
    .unwrap();
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
    let first = ensure(&mut git, "s", "/repo", &[], false, &dir, &Area::Integration);
    assert_eq!(first, Err(AreaFail::Git("fatal: scripted failure".into())));
    let second = ensure(&mut git, "s", "/repo", &[], false, &dir, &Area::Integration);
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
        ensure(&mut git, "s", "/repo", &[], false, &dir, &Area::Integration),
        Err(AreaFail::NotGit)
    );
    assert_eq!(
        ensure(&mut git, "s", "/repo", &[], false, &dir, &Area::Repo),
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
        role_dir(dir, "/repo", "director", false),
        PathBuf::from("/state/studios/acme-shop/work/integration")
    );
    assert_eq!(
        role_dir(dir, "/repo", "actor-t2-c1", false),
        PathBuf::from("/state/studios/acme-shop/work/t2")
    );
    assert_eq!(
        role_dir(dir, "/repo", "producer", false),
        PathBuf::from("/repo")
    );
}

/// Run git in `repo`; answers stdout, panics on failure. The
/// `cli_testkit::git` convention — a per-command identity so a
/// commit never asks the host's global config for one.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(["-c", "user.name=alice", "-c", "user.email=alice@host-a"])
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
    let integ = ensure(
        &mut git_run,
        "acme-shop",
        &repo_s,
        &[],
        false,
        &dir,
        &Area::Integration,
    )
    .unwrap();
    assert_eq!(integ, dir.join("work").join("integration"));
    assert_eq!(
        git(&integ, &["branch", "--show-current"]).trim(),
        "backlot/acme-shop/integration"
    );
    let t1 = ensure(
        &mut git_run,
        "acme-shop",
        &repo_s,
        &[],
        false,
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
            &[],
            false,
            &dir,
            &Area::Task("t1".into())
        ),
        Ok(t1.clone())
    );
    assert_eq!(
        ensure(
            &mut git_run,
            "acme-shop",
            &repo_s,
            &[],
            false,
            &dir,
            &Area::Integration
        ),
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
            &[],
            false,
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
    let integ = ensure(
        &mut git_run,
        "acme-shop",
        &repo_s,
        &[],
        false,
        &dir,
        &Area::Integration,
    )
    .unwrap();
    assert_eq!(integ, dir.join("work").join("integration"));
    let t1 = ensure(
        &mut git_run,
        "acme-shop",
        &repo_s,
        &[],
        false,
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
            &[],
            false,
            &dir,
            &Area::Task("t1".into())
        ),
        Ok(t1)
    );
    assert_eq!(
        ensure(
            &mut git_run,
            "acme-shop",
            &repo_s,
            &[],
            false,
            &dir,
            &Area::Integration
        ),
        Ok(integ)
    );
}

/// A workspace (D10): one worktree per member under
/// `work/<area>/<member>`, each on the shared `backlot/<s>/<name>`
/// branch in its own repository — the role's cwd is the container.
#[test]
fn a_workspace_lays_one_worktree_per_member() {
    let root = tmp();
    let ws = root.join("mono");
    let mut members = Vec::new();
    for name in ["beta-api", "web"] {
        let m = ws.join(name);
        fs::create_dir_all(&m).unwrap();
        git(&m, &["init", "-b", "main"]);
        fs::write(m.join("f"), "base\n").unwrap();
        git(&m, &["add", "."]);
        git(&m, &["commit", "-m", "base"]);
        members.push(m.to_string_lossy().into_owned());
    }
    members.sort();
    let dir = root.join("state");
    fs::create_dir_all(&dir).unwrap();
    let ws_s = ws.to_string_lossy().into_owned();
    let mut git_run = real_git;
    let integ = ensure(
        &mut git_run,
        "acme-shop",
        &ws_s,
        &members,
        false,
        &dir,
        &Area::Integration,
    )
    .unwrap();
    assert_eq!(integ, dir.join("work").join("integration"));
    let t1 = ensure(
        &mut git_run,
        "acme-shop",
        &ws_s,
        &members,
        false,
        &dir,
        &Area::Task("t1".into()),
    )
    .unwrap();
    assert_eq!(t1, dir.join("work").join("t1"));
    // Each member holds its own worktree on the shared branch name —
    // and a commit inside it lands in that member's object store.
    for name in ["beta-api", "web"] {
        let integ_m = integ.join(name);
        let t1_m = t1.join(name);
        assert!(integ_m.is_dir(), "{integ_m:?}");
        assert!(t1_m.is_dir(), "{t1_m:?}");
        assert_eq!(
            git(&integ_m, &["branch", "--show-current"]).trim(),
            "backlot/acme-shop/integration"
        );
        assert_eq!(
            git(&t1_m, &["branch", "--show-current"]).trim(),
            "backlot/acme-shop/t1"
        );
    }
    fs::write(t1.join("beta-api").join("g"), "new\n").unwrap();
    git(&t1.join("beta-api"), &["add", "."]);
    git(&t1.join("beta-api"), &["commit", "-m", "work"]);
    assert_eq!(
        git(
            &ws.join("beta-api"),
            &["log", "-1", "--format=%s", "backlot/acme-shop/t1"]
        )
        .trim(),
        "work"
    );
    assert_ne!(
        git(
            &ws.join("web"),
            &["log", "-1", "--format=%s", "backlot/acme-shop/t1"]
        )
        .trim(),
        "work"
    );
}

/// The reported collision: a workspace whose children are linked
/// worktrees of one repository plus a second repository — member
/// discovery dedups by `--git-common-dir` (D4), so `backlot/<s>/<area>`
/// is checked out once per distinct repository and the launch does
/// not fail on "already checked out" (D10).
#[test]
fn linked_worktrees_lay_one_worktree_per_distinct_repository() {
    let root = tmp();
    let ws = root.join("mono");
    let core = ws.join("zcore");
    fs::create_dir_all(&core).unwrap();
    git(&core, &["init", "-b", "main"]);
    fs::write(core.join("f"), "base\n").unwrap();
    git(&core, &["add", "."]);
    git(&core, &["commit", "-m", "base"]);
    // Two linked worktrees of `zcore` — like the reported AIFlows
    // children — plus one real second repository.
    for name in ["aa-linked", "ab-linked"] {
        git(
            &core,
            &[
                "worktree",
                "add",
                "-b",
                name,
                &ws.join(name).to_string_lossy(),
            ],
        );
    }
    let beta = ws.join("beta");
    fs::create_dir_all(&beta).unwrap();
    git(&beta, &["init", "-b", "main"]);
    fs::write(beta.join("f"), "base\n").unwrap();
    git(&beta, &["add", "."]);
    git(&beta, &["commit", "-m", "base"]);

    let members = crate::paths::member_repos(&ws);
    assert_eq!(members.len(), 2, "{members:?}");
    let dir = root.join("state");
    fs::create_dir_all(&dir).unwrap();
    let ws_s = ws.to_string_lossy().into_owned();
    let mut git_run = real_git;
    let integ = ensure(
        &mut git_run,
        "acme-shop",
        &ws_s,
        &members,
        false,
        &dir,
        &Area::Integration,
    )
    .unwrap();
    let t1 = ensure(
        &mut git_run,
        "acme-shop",
        &ws_s,
        &members,
        false,
        &dir,
        &Area::Task("t1".into()),
    )
    .unwrap();
    for member in &members {
        let leaf = Path::new(member).file_name().unwrap();
        for branch in ["integration", "t1"] {
            let area = match branch {
                "integration" => &integ,
                _ => &t1,
            };
            assert_eq!(
                git(&area.join(leaf), &["branch", "--show-current"]).trim(),
                format!("backlot/acme-shop/{branch}")
            );
        }
    }
}

/// The reported failure (D10): a workspace naming one repository
/// twice — a linked worktree listed before its main checkout — plus
/// one distinct repository lays one worktree per repository, never
/// one per member spelling, the kept checkout is the main one
/// (`core`, not the linked leaf), and the director's cwd is the
/// `work/integration` container.
#[test]
fn repeated_repository_members_count_once_for_work_areas() {
    let root = tmp();
    let ws = root.join("ws");
    // `.git` a directory marks the main checkout; a linked worktree's
    // `.git` is a gitfile.
    let main = ws.join("core");
    fs::create_dir_all(main.join(".git")).unwrap();
    let linked = ws.join("core-alpha-verify");
    fs::create_dir_all(&linked).unwrap();
    fs::write(linked.join(".git"), "gitdir: elsewhere\n").unwrap();
    let common = "/git/core/.git".to_string();
    let mut fake = FakeGit {
        inside: true,
        common_dirs: BTreeMap::from([
            (main.to_string_lossy().into_owned(), common.clone()),
            (linked.to_string_lossy().into_owned(), common),
        ]),
        ..FakeGit::default()
    };
    let mut git = |argv: &[&str], cwd: &str| fake.run(argv, cwd);
    let members = vec![
        linked.to_string_lossy().into_owned(),
        main.to_string_lossy().into_owned(),
        "/ws/embedding".into(),
    ];
    let dir = root.join("state");
    fs::create_dir_all(&dir).unwrap();
    let cwd = ensure(
        &mut git,
        "aiflows",
        &ws.to_string_lossy(),
        &members,
        false,
        &dir,
        &Area::Integration,
    )
    .unwrap();
    assert_eq!(cwd, dir.join("work").join("integration"));
    let adds: Vec<String> = fake
        .calls
        .iter()
        .filter(|c| c.contains("worktree add"))
        .cloned()
        .collect();
    assert_eq!(adds.len(), 2, "{:?}", fake.calls);
    let integration = dir.join("work").join("integration");
    assert!(
        adds.iter()
            .any(|c| c.contains(&integration.join("core").display().to_string())),
        "{adds:?}"
    );
    assert!(
        adds.iter()
            .any(|c| c.contains(&integration.join("embedding").display().to_string())),
        "{adds:?}"
    );
    assert!(
        !adds.iter().any(|c| c.contains("core-alpha-verify")),
        "{adds:?}"
    );
}

/// A member git cannot answer the common dir for keeps itself as its
/// identity — the same fallback `paths::common_dir` has — and is
/// still probed like any other: its worktree lays under its own leaf.
#[test]
fn a_member_git_cannot_answer_keeps_itself_as_identity() {
    let dir = tmp();
    let mut fake = FakeGit {
        inside: true,
        fail: Some("--git-common-dir".into()),
        ..FakeGit::default()
    };
    let mut git = |argv: &[&str], cwd: &str| fake.run(argv, cwd);
    let members = vec!["/ws/core".to_string()];
    let cwd = ensure(
        &mut git,
        "s",
        "/ws",
        &members,
        false,
        &dir,
        &Area::Integration,
    )
    .unwrap();
    assert_eq!(cwd, dir.join("work").join("integration"));
    assert!(
        fake.calls
            .iter()
            .any(|c| c == "-C /ws/core rev-parse --is-inside-work-tree"),
        "the member is still probed: {:?}",
        fake.calls
    );
    assert!(
        fake.calls.iter().any(|c| {
            c.contains("worktree add")
                && c.contains(
                    &dir.join("work")
                        .join("integration")
                        .join("core")
                        .display()
                        .to_string(),
                )
        }),
        "{:?}",
        fake.calls
    );
}

/// A no-git studio: every role's area is the bound path itself —
/// git never runs (D10).
#[test]
fn a_no_git_studio_shares_the_bound_path() {
    let dir = tmp();
    let mut fake = FakeGit::default();
    let mut git = |argv: &[&str], cwd: &str| fake.run(argv, cwd);
    for area in [Area::Repo, Area::Integration, Area::Task("t1".into())] {
        assert_eq!(
            ensure(&mut git, "s", "/plain", &[], true, &dir, &area),
            Ok(PathBuf::from("/plain"))
        );
    }
    assert!(fake.calls.is_empty(), "{:?}", fake.calls);
    assert_eq!(
        role_dir(&dir, "/plain", "director", true),
        PathBuf::from("/plain")
    );
    assert_eq!(
        role_dir(&dir, "/plain", "actor-t1-c1", true),
        PathBuf::from("/plain")
    );
}

//! The bound path's shapes (D4): classified from the filesystem —
//! the path itself first, then its direct children, never deeper —
//! plus the launch-time git initialization each shape asks for.
//!
//! `studio new` runs on the machine the studio lives on (a remote
//! picker's `studio new` goes over SSH the same way), so this module
//! only ever sees local paths — "remote" is simply where the command
//! itself ran.

use std::path::{Path, PathBuf};

use crate::cli::Fail;
use crate::workareas::{self, GitOut};

/// What the bound path is (D4).
#[derive(Debug, Clone, PartialEq)]
pub enum PathShape {
    /// The path is itself a git repository; `has_commits` tells a
    /// fresh `git init` from a usable one.
    Repo { has_commits: bool },
    /// Not a repository itself, but one or more direct children are —
    /// the member repositories, as absolute paths, sorted.
    Workspace { members: Vec<String> },
    /// A directory holding no repository at all.
    Plain,
}

/// `.git` — a directory or a worktree's `.git` file — marks the
/// directory itself as a repository, the same probe `repos` and the
/// pickers use.
///
/// The mark alone is not enough — a `.git` directory that is empty
/// or broken, or a gitfile whose pointer is dead, is no repository:
/// `is_git` is the mark AND the same `rev-parse --git-dir` the
/// launch's [`prove_git`] runs, so a pick that classified a path can
/// never meet a check that disagrees. A broken mark counts as no
/// repository — the path classifies plain and `git init` repairs it.
pub fn is_git(path: &Path) -> bool {
    path.join(".git").exists() && prove_git(&path.to_string_lossy()).is_ok()
}

/// Does the repository at `path` hold at least one commit — one
/// `rev-parse --verify HEAD`, like `repos`' `git log -1` probe.
pub fn has_commits(path: &Path) -> bool {
    let out = workareas::real_git(
        &[
            "-C",
            &path.to_string_lossy(),
            "rev-parse",
            "--verify",
            "--quiet",
            "--end-of-options",
            "HEAD",
        ],
        ".",
    );
    out.code == 0
}

/// The direct-child repositories of `path` — member candidates for a
/// workspace, absolute and sorted; never a deeper scan.
pub fn member_repos(path: &Path) -> Vec<String> {
    let mut members = Vec::new();
    if let Ok(rd) = std::fs::read_dir(path) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && is_git(&p) {
                members.push(p.to_string_lossy().into_owned());
            }
        }
    }
    members.sort();
    members
}

/// Classify `path`: a repository, a workspace over member
/// repositories, or a plain directory.
pub fn classify(path: &Path) -> PathShape {
    if is_git(path) {
        return PathShape::Repo {
            has_commits: has_commits(path),
        };
    }
    let members = member_repos(path);
    if !members.is_empty() {
        return PathShape::Workspace { members };
    }
    PathShape::Plain
}

/// The initial commit's message (D4).
pub const INITIAL_COMMIT_MSG: &str = "chore: initial commit";

/// The git arguments for one empty initial commit inside `repo` —
/// the identity is command-local `-c`, never global config. Kept as
/// data so the TUI can show exactly what launch runs and tests can
/// assert it.
pub fn initial_commit_args(repo: &Path) -> Vec<String> {
    let dir = repo.to_string_lossy().into_owned();
    vec![
        "-C".into(),
        dir,
        "-c".into(),
        "user.name=backlot".into(),
        "-c".into(),
        "user.email=backlot@localhost".into(),
        "commit".into(),
        "--allow-empty".into(),
        "-m".into(),
        INITIAL_COMMIT_MSG.into(),
    ]
}

/// The full `git init` + initial-commit argv for a plain directory.
/// `git init <path>` (not `-C`) so a directory that does not exist
/// yet is created by git itself.
pub fn init_args(path: &Path) -> Vec<Vec<String>> {
    vec![
        vec!["init".into(), path.to_string_lossy().into_owned()],
        initial_commit_args(path),
    ]
}

/// What the path check settled: the member repositories a workspace
/// binds, and whether the studio runs with no git at all (D4). A
/// plain directory the user asked to `--init` arrives as an empty
/// `members` — it becomes a repository at launch.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Binding {
    pub members: Vec<String>,
    pub no_git: bool,
}

/// `git rev-parse --git-dir` proves a repository the user owns —
/// linked worktrees count alike.
fn prove_git(repo: &str) -> Result<(), Fail> {
    let out = workareas::real_git(&["rev-parse", "--git-dir", "--end-of-options"], repo);
    if out.code != 0 {
        return Err(Fail::refused(format!(
            "'{repo}' is not a git repository ({})",
            workareas::err_line(&out)
        )));
    }
    Ok(())
}

/// The path-shape check the draft and the launch share: classify
/// `repo`, prove what git must already be true, and apply the flag
/// rules — a plain directory without a choice refuses with both
/// options named, and a flag that fights the shape refuses too. Pure
/// checks only — nothing is created.
pub fn check(repo: &str, init: bool, no_git: bool) -> Result<Binding, Fail> {
    if init && no_git {
        return Err(Fail::usage("give --init or --no-git, not both"));
    }
    let path = Path::new(repo);
    match classify(path) {
        PathShape::Repo { has_commits } => {
            if no_git {
                return Err(Fail::refused(format!(
                    "--no-git does not apply: '{repo}' is a git repository"
                )));
            }
            if init && has_commits {
                return Err(Fail::refused(format!(
                    "--init has no effect: '{repo}' is already a git repository"
                )));
            }
            prove_git(repo)?;
            Ok(Binding::default())
        }
        PathShape::Workspace { members } => {
            if init {
                return Err(Fail::refused(format!(
                    "--init does not apply: '{repo}' is a workspace of member repositories — \
                     no parent repository is ever created"
                )));
            }
            if no_git {
                return Err(Fail::refused(format!(
                    "--no-git does not apply: '{repo}' holds member repositories"
                )));
            }
            for member in &members {
                prove_git(member)?;
            }
            Ok(Binding {
                members,
                no_git: false,
            })
        }
        PathShape::Plain => {
            if init {
                return Ok(Binding::default());
            }
            if no_git {
                return Ok(Binding {
                    members: Vec::new(),
                    no_git: true,
                });
            }
            Err(Fail::refused(format!(
                "--repo '{repo}' holds no git repository — pass --init to create one with an \
                 empty initial commit, or --no-git to bind it without git (reduced isolation)"
            )))
        }
    }
}

fn run_git(args: &[String], cwd: &str) -> GitOut {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    workareas::real_git(&refs, cwd)
}

/// One empty initial commit in `repo` — idempotent: a repository
/// that already holds commits is left alone.
fn ensure_initial_commit(repo: &Path) -> Result<(), Fail> {
    if has_commits(repo) {
        return Ok(());
    }
    let out = run_git(&initial_commit_args(repo), &repo.to_string_lossy());
    if out.code != 0 {
        return Err(Fail::refused(format!(
            "cannot create the initial commit in {} ({})",
            repo.display(),
            workareas::err_line(&out)
        )));
    }
    Ok(())
}

/// The launch-time initialization the shape asks for (D4): a plain
/// directory bound `--init` becomes a repository with one empty
/// commit; an empty repository — the bound path or any workspace
/// member — gets the same empty initial commit; a no-git binding
/// touches nothing.
pub fn initialize(repo: &str, members: &[String], no_git: bool, init: bool) -> Result<(), Fail> {
    if no_git {
        return Ok(());
    }
    let root = PathBuf::from(repo);
    if init {
        for args in init_args(&root) {
            let out = run_git(&args, &root.to_string_lossy());
            if out.code != 0 {
                return Err(Fail::refused(format!(
                    "git init failed in {repo} ({})",
                    workareas::err_line(&out)
                )));
            }
        }
        return Ok(());
    }
    if members.is_empty() {
        // A bound path that is no repository — a path that does not
        // exist yet, or one the flag rules already let through:
        // nothing to initialize; the stage's `not a git repository`
        // attention names it at the first role start. A workspace
        // root is never a repository itself — its members are.
        if is_git(&root) {
            ensure_initial_commit(&root)?;
        }
        return Ok(());
    }
    for member in members {
        ensure_initial_commit(Path::new(member))?;
    }
    Ok(())
}

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
use crate::driver::{LocalDriver, MachineDriver};
use crate::workareas::{self, GitOut, GitRun};

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
    is_git_with(&LocalDriver::new(), path)
}

/// [`is_git`] through a driver's git runner.
pub fn is_git_with(driver: &dyn MachineDriver, path: &Path) -> bool {
    path.join(".git").exists() && prove_git(driver, &path.to_string_lossy()).is_ok()
}

/// Does the repository at `path` hold at least one commit — one
/// `rev-parse --verify HEAD`, like `repos`' `git log -1` probe.
pub fn has_commits_with(driver: &dyn MachineDriver, path: &Path) -> bool {
    let mut git = driver.git();
    commits_held(&mut **git, path)
}

/// The one `rev-parse --verify HEAD` probe, through a runner.
fn commits_held(git: &mut GitRun<'_>, path: &Path) -> bool {
    let out = git(
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

/// The repository a member path stands for — `rev-parse
/// --git-common-dir` resolved canonically: every linked worktree of
/// one repository answers the same directory, so members dedup by it
/// and a per-member worktree can never collide on the one checkout a
/// branch allows (D4, D10). `--path-format=absolute` makes the main
/// checkout answer an absolute path like a linked worktree does, and
/// canonicalizing then lands both on one spelling — a symlinked
/// parent (`/var` → `/private/var` on macOS) cannot split them. A
/// path git cannot answer keeps itself as its identity — it is
/// reported by the checks that probe it.
fn common_dir(git: &mut GitRun<'_>, path: &Path) -> PathBuf {
    let out = git(
        &[
            "-C",
            &path.to_string_lossy(),
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
            "--end-of-options",
        ],
        ".",
    );
    if out.code != 0 {
        return path.to_path_buf();
    }
    let raw = PathBuf::from(out.stdout.trim());
    let abs = match raw.is_absolute() {
        true => raw,
        false => path.join(raw),
    };
    abs.canonicalize().unwrap_or(abs)
}

/// One path per repository — `common_dir` is the identity, so linked
/// worktrees collapse to a single member; the main checkout (`.git` a
/// directory, not a worktree's gitfile) wins the slot (D4, D10).
fn distinct(git: &mut GitRun<'_>, paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut members: Vec<(PathBuf, PathBuf)> = Vec::new();
    for p in paths {
        let id = common_dir(git, &p);
        match members.iter_mut().find(|(mid, _)| *mid == id) {
            None => members.push((id, p)),
            Some((_, stored)) if p.join(".git").is_dir() && !stored.join(".git").is_dir() => {
                *stored = p;
            }
            Some(_) => {}
        }
    }
    members.into_iter().map(|(_, p)| p).collect()
}

/// The direct-child repositories of `path` — member candidates for a
/// workspace, absolute and sorted; never a deeper scan. Members are
/// distinct repositories: linked worktrees of one repository count
/// once (D4).
pub fn member_repos(path: &Path) -> Vec<String> {
    member_repos_with(&LocalDriver::new(), path)
}

/// [`member_repos`] through a driver's git runner.
pub fn member_repos_with(driver: &dyn MachineDriver, path: &Path) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Ok(rd) = std::fs::read_dir(path) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && is_git_with(driver, &p) {
                candidates.push(p);
            }
        }
    }
    candidates.sort();
    let mut git = driver.git();
    let mut members = distinct(&mut **git, candidates);
    members.sort();
    members
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

/// Classify `path`: a repository, a workspace over member
/// repositories, or a plain directory.
pub fn classify(path: &Path) -> PathShape {
    classify_with(&LocalDriver::new(), path)
}

/// [`classify`] through a driver's git runner.
pub fn classify_with(driver: &dyn MachineDriver, path: &Path) -> PathShape {
    if is_git_with(driver, path) {
        return PathShape::Repo {
            has_commits: has_commits_with(driver, path),
        };
    }
    let members = member_repos_with(driver, path);
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
fn prove_git(driver: &dyn MachineDriver, repo: &str) -> Result<(), Fail> {
    let mut git = driver.git();
    let out = (**git)(&["rev-parse", "--git-dir", "--end-of-options"], repo);
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
    check_with(&LocalDriver::new(), repo, init, no_git)
}

/// [`check`] through a driver's git runner.
pub fn check_with(
    driver: &dyn MachineDriver,
    repo: &str,
    init: bool,
    no_git: bool,
) -> Result<Binding, Fail> {
    if init && no_git {
        return Err(Fail::usage("give --init or --no-git, not both"));
    }
    let path = Path::new(repo);
    match classify_with(driver, path) {
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
            prove_git(driver, repo)?;
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
                prove_git(driver, member)?;
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

fn run_git(git: &mut GitRun<'_>, args: &[String], cwd: &str) -> GitOut {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    git(&refs, cwd)
}

/// One empty initial commit in `repo` — idempotent: a repository
/// that already holds commits is left alone.
fn ensure_initial_commit(git: &mut GitRun<'_>, repo: &Path) -> Result<(), Fail> {
    if commits_held(git, repo) {
        return Ok(());
    }
    let out = run_git(git, &initial_commit_args(repo), &repo.to_string_lossy());
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
    initialize_with(&LocalDriver::new(), repo, members, no_git, init)
}

/// [`initialize`] through a driver's git runner.
pub fn initialize_with(
    driver: &dyn MachineDriver,
    repo: &str,
    members: &[String],
    no_git: bool,
    init: bool,
) -> Result<(), Fail> {
    if no_git {
        return Ok(());
    }
    let root = PathBuf::from(repo);
    if init {
        let mut git = driver.git();
        for args in init_args(&root) {
            let out = run_git(&mut **git, &args, &root.to_string_lossy());
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
        if is_git_with(driver, &root) {
            let mut git = driver.git();
            ensure_initial_commit(&mut **git, &root)?;
        }
        return Ok(());
    }
    let mut git = driver.git();
    for member in distinct(&mut **git, members.iter().map(PathBuf::from)) {
        ensure_initial_commit(&mut **git, &member)?;
    }
    Ok(())
}

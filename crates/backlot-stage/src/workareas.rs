//! Actor work areas (D10): a pure plan over an injected git runner,
//! so tests record calls and production shells out to `git`.
//!
//! The studio repo must be a git work tree. The director works in
//! `<studio dir>/work/integration` on `backlot/<studio>/integration`,
//! branched from the repo's `HEAD`; an actor on task `<t>` works in
//! `<studio dir>/work/<t>` on `backlot/<studio>/<t>`, branched from
//! the integration branch's current tip. Producer and writer stay in
//! the repo. Every step is idempotent: an existing worktree or
//! branch is reused, so a recast continues on top and a reopened
//! person keeps their folder. Worktrees stay after a task closes —
//! reopening needs them.

use std::path::{Path, PathBuf};

use backlot_model::state::actor_task;

use crate::reconcile::{RoleKind, Start};

/// What one git invocation produced.
#[derive(Debug, Clone, PartialEq)]
pub struct GitOut {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// How the plan runs git: argv and cwd in, the process result out.
/// Injected so tests never spawn git; production uses [`real_git`].
pub type GitRun<'a> = dyn FnMut(&[&str], &str) -> GitOut + 'a;

/// Production [`GitRun`]: spawn `git`, capture status and both pipes.
pub fn real_git(argv: &[&str], cwd: &str) -> GitOut {
    match std::process::Command::new("git")
        .args(argv)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(o) => GitOut {
            code: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        },
        Err(e) => GitOut {
            code: -1,
            stdout: String::new(),
            stderr: format!("git: {e}"),
        },
    }
}

/// Which work area a role needs.
#[derive(Debug, Clone, PartialEq)]
pub enum Area {
    /// Producer and writer: the user's repo itself, no git needed.
    Repo,
    /// The director: `work/integration` on `backlot/<s>/integration`.
    Integration,
    /// An actor: `work/<task>` on `backlot/<s>/<task>`.
    Task(String),
}

/// The attention `ensure` failure records.
#[derive(Debug, Clone, PartialEq)]
pub enum AreaFail {
    /// The studio repo is not a git work tree.
    NotGit,
    /// A git command failed; detail is its first stderr line.
    Git(String),
}

/// Which work area a start needs — the pure `Start` → [`Area`] map.
pub fn area_of(start: &Start) -> Area {
    match start.kind {
        RoleKind::Director => Area::Integration,
        RoleKind::Actor => Area::Task(
            start
                .task
                .clone()
                .or_else(|| actor_task(&start.role).map(String::from))
                .unwrap_or_else(|| start.role.clone()),
        ),
        RoleKind::Producer | RoleKind::Writer => Area::Repo,
    }
}

/// The folder a role's terminal is expected to run in — the same
/// path `ensure` produces, without the git calls. The delivery gate
/// and the trust-dialog check compare against it.
pub fn role_dir(dir: &Path, repo: &str, role: &str) -> PathBuf {
    match role {
        "director" => dir.join("work").join("integration"),
        _ => match actor_task(role) {
            Some(task) => dir.join("work").join(task),
            None => PathBuf::from(repo),
        },
    }
}

/// stderr's most useful line — the `fatal:`/`error:` when git
/// prints one (progress noise like `Preparing worktree` comes
/// first), else the first non-blank line as before.
pub(crate) fn err_line(out: &GitOut) -> String {
    out.stderr
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("fatal:") || l.starts_with("error:"))
        .or_else(|| out.stderr.lines().map(str::trim).find(|l| !l.is_empty()))
        .map(String::from)
        .unwrap_or_else(|| format!("exit {}", out.code))
}

fn fail_on(out: &GitOut) -> Result<(), AreaFail> {
    match out.code {
        0 => Ok(()),
        _ => Err(AreaFail::Git(err_line(out))),
    }
}

/// `worktree list --porcelain` holds a block per worktree: the
/// `worktree <dir>` head then `branch refs/heads/<name>`. Reuse asks
/// for both — a dir checked out on another branch is not this area.
/// git prints the dir resolved, so the looked-for path resolves too
/// — a state dir reached through a symlink still matches; a dir
/// that does not exist yet cannot be listed.
pub fn worktree_listed(list: &str, dir: &Path, branch: &str) -> bool {
    let Ok(dir) = dir.canonicalize() else {
        return false;
    };
    let want = format!("worktree {}", dir.display());
    let want_branch = format!("branch refs/heads/{branch}");
    let mut at = false;
    for line in list.lines() {
        if line.starts_with("worktree ") {
            at = line == want;
        } else if at && line == want_branch {
            return true;
        }
    }
    false
}

/// One worktree, creating its branch when needed. `base` is the
/// branch point for a new branch (`HEAD` for the integration one).
fn ensure_worktree(
    git: &mut GitRun<'_>,
    studio: &str,
    repo: &str,
    dir: &Path,
    name: &str,
    base: Option<&str>,
) -> Result<PathBuf, AreaFail> {
    let work = dir.join("work").join(name);
    let branch = format!("backlot/{studio}/{name}");
    let list = git(&["-C", repo, "worktree", "list", "--porcelain"], repo);
    fail_on(&list)?;
    if worktree_listed(&list.stdout, &work, &branch) {
        return Ok(work);
    }
    let heads = format!("refs/heads/{branch}");
    let verify = git(
        &["-C", repo, "rev-parse", "--verify", "--quiet", &heads],
        repo,
    );
    let work_s = work.to_string_lossy().into_owned();
    let add = if verify.code == 0 {
        // The branch already exists — a recast or a pruned worktree:
        // attach it, work continues on top.
        git(&["-C", repo, "worktree", "add", &work_s, &branch], repo)
    } else {
        git(
            &[
                "-C",
                repo,
                "worktree",
                "add",
                "-b",
                &branch,
                &work_s,
                base.unwrap_or("HEAD"),
            ],
            repo,
        )
    };
    fail_on(&add)?;
    Ok(work)
}

/// Make sure the area exists and answer the cwd the terminal gets.
/// `Repo` needs nothing; the others first prove the studio repo is a
/// git work tree — `NotGit` starts no actor or director.
pub fn ensure(
    git: &mut GitRun<'_>,
    studio: &str,
    repo: &str,
    dir: &Path,
    area: &Area,
) -> Result<PathBuf, AreaFail> {
    if matches!(area, Area::Repo) {
        return Ok(PathBuf::from(repo));
    }
    let probe = git(&["-C", repo, "rev-parse", "--is-inside-work-tree"], repo);
    if probe.code != 0 || probe.stdout.trim() != "true" {
        return Err(AreaFail::NotGit);
    }
    // Task areas branch off the integration tip, so it exists first —
    // even when the director has not started yet.
    let integration = ensure_worktree(git, studio, repo, dir, "integration", None)?;
    match area {
        Area::Integration => Ok(integration),
        Area::Task(task) => ensure_worktree(
            git,
            studio,
            repo,
            dir,
            task,
            Some(&format!("backlot/{studio}/integration")),
        ),
        Area::Repo => unreachable!(),
    }
}

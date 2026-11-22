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

/// The git seam and the production runner live in `gitcmd` (bounded);
/// re-exported here so every work-area caller keeps one path.
pub use crate::gitcmd::{GitOut, GitRun, real_git};

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
/// and the trust-dialog check compare against it. A studio bound
/// with no git shares the bound path between every role (D10).
pub fn role_dir(dir: &Path, repo: &str, role: &str, no_git: bool) -> PathBuf {
    if no_git {
        return PathBuf::from(repo);
    }
    match role {
        "director" => dir.join("work").join("integration"),
        _ => match actor_task(role) {
            Some(task) => dir.join("work").join(task),
            None => PathBuf::from(repo),
        },
    }
}

/// stderr's most useful line — a timeout first (the actionable
/// `git timed out after …`), then the `fatal:`/`error:` when git
/// prints one (progress noise like `Preparing worktree` comes first),
/// else the first non-blank line as before.
pub(crate) fn err_line(out: &GitOut) -> String {
    out.stderr
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("git timed out after"))
        .or_else(|| {
            out.stderr
                .lines()
                .map(str::trim)
                .find(|l| l.starts_with("fatal:") || l.starts_with("error:"))
        })
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

/// A member repository's leaf name under `work/<area>/` — the
/// directory a workspace lays one worktree per distinct repository
/// into (D10).
fn member_leaf(member: &str) -> String {
    Path::new(member)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| member.to_string())
}

/// The repository a member path stands for — `rev-parse
/// --git-common-dir` through the injected runner, canonicalized when
/// possible: every linked worktree of one repository answers the same
/// directory. `--path-format=absolute` makes the main checkout answer
/// an absolute path like a linked worktree does, so canonicalizing
/// lands both on one spelling — a symlinked parent cannot split
/// them. A path git cannot answer keeps itself as its identity — the
/// probes below still report it. Mirrors `paths::common_dir`, which
/// shells out directly and cannot drive the fake seam.
fn member_common_dir(git: &mut GitRun<'_>, member: &str) -> PathBuf {
    let out = git(
        &[
            "-C",
            member,
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
            "--end-of-options",
        ],
        member,
    );
    if out.code != 0 {
        return PathBuf::from(member);
    }
    let raw = PathBuf::from(out.stdout.trim());
    let abs = if raw.is_absolute() {
        raw
    } else {
        Path::new(member).join(raw)
    };
    abs.canonicalize().unwrap_or(abs)
}

/// One member per repository (D10): the identity is the member's
/// common dir, so a workspace that names one repository twice — a
/// checkout and its linked worktrees — lays its per-member worktree
/// once and can never collide on the one checkout a branch allows.
/// The main checkout (`.git` a directory, not a worktree's gitfile)
/// wins the slot, keeping the `work/<area>/<member-leaf>` names
/// sensible. Mirrors `paths::distinct` over the injected runner; a
/// member git cannot answer keeps itself as its identity and is
/// still probed below.
fn distinct_members<'a>(git: &mut GitRun<'_>, members: &'a [String]) -> Vec<&'a str> {
    let mut kept: Vec<(PathBuf, &'a str, bool)> = Vec::new();
    for member in members {
        let id = member_common_dir(git, member);
        let main = Path::new(member).join(".git").is_dir();
        match kept.iter_mut().find(|(mid, ..)| *mid == id) {
            None => kept.push((id, member.as_str(), main)),
            Some((_, stored, stored_main)) if main && !*stored_main => {
                *stored = member.as_str();
                *stored_main = main;
            }
            Some(_) => {}
        }
    }
    kept.into_iter().map(|(_, m, _)| m).collect()
}

/// One worktree, creating its branch when needed. `path_name` is the
/// directory under `work/`; `branch_name` is the `backlot/<studio>/`
/// suffix — for a workspace member the two differ, since every member
/// shares one branch naming scheme. `base` is the branch point for a
/// new branch (`HEAD` for the integration one).
fn ensure_worktree(
    git: &mut GitRun<'_>,
    studio: &str,
    repo: &str,
    dir: &Path,
    path_name: &str,
    branch_name: &str,
    base: Option<&str>,
) -> Result<PathBuf, AreaFail> {
    let work = dir.join("work").join(path_name);
    let branch = format!("backlot/{studio}/{branch_name}");
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
/// `Repo` needs nothing; the others first prove every repository in
/// play is a git work tree — `NotGit` starts no actor or director.
/// `members` is empty for a single-repository studio; a workspace
/// lays one worktree per distinct repository under
/// `work/<area>/<member-leaf>` and the role's cwd is the
/// `work/<area>` directory itself (D10). Members naming one
/// repository twice — a checkout and its linked worktrees — count
/// once, preferring the main checkout. A studio bound with no git
/// shares the bound path between every role.
pub fn ensure(
    git: &mut GitRun<'_>,
    studio: &str,
    repo: &str,
    members: &[String],
    no_git: bool,
    dir: &Path,
    area: &Area,
) -> Result<PathBuf, AreaFail> {
    if matches!(area, Area::Repo) || no_git {
        return Ok(PathBuf::from(repo));
    }
    let repos: Vec<&str> = match members.is_empty() {
        true => vec![repo],
        false => distinct_members(git, members),
    };
    for member in &repos {
        let probe = git(
            &["-C", member, "rev-parse", "--is-inside-work-tree"],
            member,
        );
        if probe.code != 0 || probe.stdout.trim() != "true" {
            return Err(AreaFail::NotGit);
        }
    }
    // Task areas branch off the integration tip, so it exists first —
    // even when the director has not started yet. Per member in a
    // workspace; the single-repository layout keeps `work/<name>`.
    let workspace = !members.is_empty();
    for member in &repos {
        let path = match workspace {
            true => format!("integration/{}", member_leaf(member)),
            false => "integration".to_string(),
        };
        ensure_worktree(git, studio, member, dir, &path, "integration", None)?;
    }
    match area {
        Area::Integration => Ok(dir.join("work").join("integration")),
        Area::Task(task) => {
            for member in &repos {
                let path = match workspace {
                    true => format!("{task}/{}", member_leaf(member)),
                    false => task.clone(),
                };
                ensure_worktree(
                    git,
                    studio,
                    member,
                    dir,
                    &path,
                    task,
                    Some(&format!("backlot/{studio}/integration")),
                )?;
            }
            Ok(dir.join("work").join(task))
        }
        Area::Repo => unreachable!(),
    }
}

//! The picker's real seams: `read_dir` for children, `git` for the
//! repository mark — everything [`Seams`] asks a live filesystem.

use std::path::{Path, PathBuf};

use super::tree::{Repo, Seams};
use crate::workareas;

/// The picker's `~` — `$HOME`, `/` as the last resort.
pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// The production seams — real `read_dir` and real `git`.
pub fn real() -> Seams<'static> {
    Seams {
        lister: Box::new(list_dirs),
        repo: Box::new(probe),
        is_dir: Box::new(|p| p.is_dir()),
        is_repo: Box::new(crate::paths::is_git),
    }
}

/// `path`'s child directories — `None` when it cannot be read, so
/// the picker shows `(no access)` and goes on. A symlinked
/// directory counts.
pub(crate) fn list_dirs(path: &Path) -> Option<Vec<PathBuf>> {
    let rd = std::fs::read_dir(path).ok()?;
    let mut dirs = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            dirs.push(p);
        }
    }
    Some(dirs)
}

/// `root`'s current branch — one `symbolic-ref`; `None` without one
/// (detached, or not a repository at all). The in-use group uses it to
/// spot backlot's own `backlot/<studio>/…` branches (U7 fix 2).
pub(crate) fn branch(path: &Path) -> Option<String> {
    let out = workareas::real_git(
        &[
            "-C",
            &path.to_string_lossy(),
            "symbolic-ref",
            "--quiet",
            "--short",
            "HEAD",
        ],
        ".",
    );
    (out.code == 0)
        .then(|| out.stdout.trim().to_string())
        .filter(|branch| !branch.is_empty())
}

/// The directory's shape as a [`Repo`]: a `.git` git itself accepts
/// is the gate — children inside a repository's work tree are not
/// repositories, and a broken mark counts as no repository — then
/// one `log` for the commit age and one `symbolic-ref` for the
/// branch (`detached` when there is none). A repository with no
/// commit yet is [`Repo::Empty`]. Without an accepted `.git` the
/// children answer the shape: direct-child repositories make a
/// [`Repo::Workspace`], a readable rest a [`Repo::Dir`], an
/// unreadable or missing path [`Repo::Missing`].
pub(crate) fn probe(path: &Path) -> Repo {
    if !crate::paths::is_git(path) {
        if list_dirs(path).is_none() {
            return Repo::Missing;
        }
        return match crate::paths::member_repos(path).len() {
            0 => Repo::Dir,
            n => Repo::Workspace { repos: n },
        };
    }
    let dir = path.to_string_lossy();
    let log = workareas::real_git(&["-C", &dir, "log", "-1", "--format=%ct"], ".");
    if log.code != 0 {
        return Repo::Empty;
    }
    let age_s = log
        .stdout
        .trim()
        .parse::<u64>()
        .map(|then| now().saturating_sub(then))
        .unwrap_or(0);
    let branch = branch(path).unwrap_or_else(|| "detached".to_string());
    Repo::Commit { branch, age_s }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

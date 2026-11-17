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
        is_repo: Box::new(|p| p.join(".git").exists()),
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

/// `git log -1` as a [`Repo`]: `.git` itself is the gate — children
/// inside a repository's work tree are not repositories — then one
/// `log` for the commit age and one `symbolic-ref` for the branch
/// (`detached` when there is none). A repository with no commit yet
/// is [`Repo::Empty`].
pub(crate) fn probe(path: &Path) -> Repo {
    if !path.join(".git").exists() {
        return Repo::None;
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
    let out = workareas::real_git(
        &["-C", &dir, "symbolic-ref", "--quiet", "--short", "HEAD"],
        ".",
    );
    let branch = match out.stdout.trim() {
        b if out.code == 0 && !b.is_empty() => b.to_string(),
        _ => "detached".to_string(),
    };
    Repo::Commit { branch, age_s }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

//! The "repositories in use" group's pure half: each live
//! terminal's cwd walked up to its enclosing repository root —
//! the nearest one, a nested repository wins over its parent —
//! deduplicated in first-seen order. It is the picker's fastest
//! path, because most studios are made for a repository an agent
//! already works in.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// `cwd` walked up to the nearest ancestor — itself included —
/// that `is_repo` calls a repository root; `None` when none is.
pub fn repo_root(cwd: &Path, is_repo: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    cwd.ancestors().find(|a| is_repo(a)).map(Path::to_path_buf)
}

/// Each cwd's [`repo_root`], deduplicated in first-seen order —
/// several terminals in one repository give one row. The picker's
/// cap of eight applies when it is built.
///
/// backlot's own worktrees stay out (see `backlot_own`): `state_dir`
/// is its state base, resolved as the stage resolves it, and `branch`
/// reads a repository's current branch.
pub fn collect(
    cwds: Vec<PathBuf>,
    state_dir: &Path,
    is_repo: &dyn Fn(&Path) -> bool,
    branch: &dyn Fn(&Path) -> Option<String>,
) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for cwd in cwds {
        if let Some(root) = repo_root(&cwd, is_repo)
            && seen.insert(root.clone())
            && !backlot_own(&root, state_dir, branch)
        {
            out.push(root);
        }
    }
    out
}

/// A role's worktree, which nobody starts a studio from: it sits under
/// backlot's state dir, or — for a remote core, whose state dir is not
/// this one — on a branch in backlot's `backlot/<studio>/…` namespace.
/// The state-dir test comes first, so it costs no git call.
fn backlot_own(root: &Path, state_dir: &Path, branch: &dyn Fn(&Path) -> Option<String>) -> bool {
    root.starts_with(state_dir) || branch(root).is_some_and(|b| b.starts_with("backlot/"))
}

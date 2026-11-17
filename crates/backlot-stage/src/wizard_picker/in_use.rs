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
pub fn collect(cwds: Vec<PathBuf>, is_repo: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for cwd in cwds {
        if let Some(root) = repo_root(&cwd, is_repo)
            && seen.insert(root.clone())
        {
            out.push(root);
        }
    }
    out
}

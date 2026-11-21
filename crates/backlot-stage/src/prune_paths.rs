//! Path scoping for `studio prune`: which of a listed worktree's
//! paths belong to the studio's `work/` tree, and how they read in
//! the output. Symlinked state directories (macOS `/var` →
//! `/private/var`) make git's spelling and the state dir's differ, so
//! both sides are compared canonically — a vanished worktree's folder
//! cannot be canonicalized, so its parent takes that role.

use std::path::{Path, PathBuf};

/// Canonicalize `path`, resolving a symlinked parent even when the leaf
/// itself is gone. `Path::canonicalize` refuses a missing leaf, so a
/// pruned worktree folder falls back to its parent plus file name.
fn canonicalize_with_missing_leaf(path: &Path) -> Option<PathBuf> {
    if let Ok(p) = path.canonicalize() {
        return Some(p);
    }
    let parent = path.parent()?;
    let name = path.file_name()?;
    Some(parent.canonicalize().ok()?.join(name))
}

/// `true` when `path` sits strictly under `root`, with symlinked state
/// directories resolved on both sides.
pub(crate) fn under(path: &Path, root: &Path) -> bool {
    let strictly_under = |p: &Path, r: &Path| p != r && p.starts_with(r);
    if strictly_under(path, root) {
        return true;
    }
    // macOS resolves `/var` to `/private/var`: git prints the real
    // path while the state dir (and a worktree whose folder is gone)
    // still carries the symlink. Compare the raw path against the
    // canonical root first, so a deleted tree still matches.
    if let Some(canon_root) = canonicalize_with_missing_leaf(root) {
        if strictly_under(path, &canon_root) {
            return true;
        }
        if let Some(canon_path) = canonicalize_with_missing_leaf(path) {
            return strictly_under(&canon_path, &canon_root);
        }
    }
    false
}

/// A path as it reads inside the studio directory when it can —
/// trying the spelled directory, its canonical form (macOS
/// `/var` → `/private/var`), and the canonical path as a last resort.
pub(crate) fn shown(path: &Path, dir: &Path) -> String {
    if let Ok(rel) = path.strip_prefix(dir) {
        return rel.display().to_string();
    }
    if let Some(canon_dir) = canonicalize_with_missing_leaf(dir) {
        if let Ok(rel) = path.strip_prefix(&canon_dir) {
            return rel.display().to_string();
        }
        if let Some(canon_path) = canonicalize_with_missing_leaf(path)
            && let Ok(rel) = canon_path.strip_prefix(&canon_dir)
        {
            return rel.display().to_string();
        }
    }
    path.display().to_string()
}

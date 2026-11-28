//! Path resolution for the provably read-only shell classifier.

use std::path::{Component, Path, PathBuf};

fn expand_tilde(arg: &str, home: Option<&Path>) -> Result<PathBuf, &'static str> {
    if arg == "~" || arg.starts_with("~/") {
        let home = home.ok_or("a tilde path cannot be resolved")?;
        return Ok(home.join(
            arg.strip_prefix('~')
                .unwrap_or_default()
                .trim_start_matches('/'),
        ));
    }
    if arg.starts_with('~') {
        return Err("another user's tilde path is outside the role");
    }
    Ok(PathBuf::from(arg))
}

/// Resolve `path` component by component. Every existing prefix is
/// canonicalized, so a symlink is compared by its target; a missing
/// tail is compared against its deepest existing parent.
pub(crate) fn resolve_path(path: &Path) -> PathBuf {
    let mut cur = if path.is_absolute() {
        PathBuf::from("/")
    } else {
        return path.to_path_buf();
    };
    let mut missing = false;
    for comp in path.components() {
        match comp {
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                if missing {
                    cur.pop();
                } else {
                    cur.push("..");
                    if cur.exists() {
                        cur = cur.canonicalize().unwrap_or_else(|_| cur.clone());
                    } else {
                        cur.pop();
                        cur.pop();
                        missing = true;
                    }
                }
            }
            Component::Normal(part) => {
                cur.push(part);
                if !missing && cur.exists() {
                    cur = cur.canonicalize().unwrap_or_else(|_| cur.clone());
                } else {
                    missing = true;
                }
            }
            Component::Prefix(_) => {}
        }
    }
    cur
}

fn resolve_arg(arg: &str, cwd: &Path, home: Option<&Path>) -> Result<PathBuf, &'static str> {
    if arg.is_empty() || arg.contains('\0') {
        return Err("an empty or NUL path cannot be checked");
    }
    let raw = expand_tilde(arg, home)?;
    let joined = if raw.is_absolute() {
        raw
    } else {
        cwd.join(raw)
    };
    if !joined.is_absolute() {
        return Err("a relative path cannot be checked");
    }
    Ok(resolve_path(&joined))
}

fn canonical_root(root: &Path) -> PathBuf {
    let absolute = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(root)
    };
    resolve_path(&absolute)
}

/// Check one filesystem path argument against the role's roots and the
/// conservative secret-path table.
pub(crate) fn check_path(
    arg: &str,
    cwd: &Path,
    roots: &[PathBuf],
    home: Option<&Path>,
) -> Result<(), &'static str> {
    let resolved = resolve_arg(arg, cwd, home)?;
    if crate::permit_cmd::secret_path(&resolved) {
        return Err("path names a secret");
    }
    if roots.iter().any(|root| {
        let root = canonical_root(root);
        resolved == root || resolved.starts_with(root)
    }) {
        Ok(())
    } else {
        Err("path is outside the role's roots")
    }
}

/// Whether a word is shaped like a path even in a position that may
/// instead be a pattern, revision or operand.
pub(crate) fn obvious_path(word: &str) -> bool {
    word == "~"
        || word == "."
        || word == ".."
        || word.starts_with('/')
        || word.starts_with("~/")
        || word.starts_with("./")
        || word.starts_with("../")
        || word.contains('/')
}

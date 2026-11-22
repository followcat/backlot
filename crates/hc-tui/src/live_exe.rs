//! Resolve the live `backlot` executable path for TUI subprocess spawns.
//!
//! On Linux, replacing `/home/user/.local/bin/backlot` while a TUI client is
//! already open causes `std::env::current_exe()` to report
//! `"/home/user/.local/bin/backlot (deleted)"`. Stripping the suffix when the
//! replacement exists on disk keeps actions working across upgrades, and
//! falling back to `"backlot"` lets `PATH` resolve moved/worktree binaries.

use std::path::{Path, PathBuf};

const DELETED_SUFFIX: &str = " (deleted)";

/// Pure path resolver for the running `backlot` executable.
pub(crate) fn resolve_live_exe(current: Option<&Path>, exists: impl Fn(&Path) -> bool) -> PathBuf {
    if let Some(p) = current {
        let text = p.to_string_lossy();
        if let Some(stripped) = text.strip_suffix(DELETED_SUFFIX) {
            let stripped_path = PathBuf::from(stripped);
            if exists(&stripped_path) {
                return stripped_path;
            }
            if exists(p) {
                return p.to_path_buf();
            }
        } else if exists(p) {
            return p.to_path_buf();
        }
    }
    PathBuf::from("backlot")
}

/// The live `backlot` binary path for local subprocess runners.
pub(crate) fn live_exe() -> PathBuf {
    let current = std::env::current_exe().ok();
    resolve_live_exe(current.as_deref(), |p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_existing_binary_is_returned_verbatim() {
        let p = Path::new("/home/user/.local/bin/backlot");
        assert_eq!(
            resolve_live_exe(Some(p), |q| q == p),
            PathBuf::from("/home/user/.local/bin/backlot")
        );
    }

    #[test]
    fn linux_deleted_exe_resolves_to_replacement_when_present() {
        let current = Path::new("/home/user/.local/bin/backlot (deleted)");
        let replacement = Path::new("/home/user/.local/bin/backlot");
        assert_eq!(
            resolve_live_exe(Some(current), |q| q == replacement),
            PathBuf::from("/home/user/.local/bin/backlot")
        );
    }

    #[test]
    fn deleted_worktree_binary_falls_back_to_backlot_on_path() {
        let current = Path::new("/tmp/wt/target/release/backlot (deleted)");
        assert_eq!(
            resolve_live_exe(Some(current), |_| false),
            PathBuf::from("backlot")
        );
    }

    #[test]
    fn missing_current_exe_falls_back_to_backlot() {
        assert_eq!(resolve_live_exe(None, |_| true), PathBuf::from("backlot"));
        let non_deleted_missing = Path::new("/home/user/.local/bin/backlot");
        assert_eq!(
            resolve_live_exe(Some(non_deleted_missing), |_| false),
            PathBuf::from("backlot")
        );
    }

    #[test]
    fn literal_deleted_filename_returns_itself_when_stripped_missing() {
        let current = Path::new("/home/user/bin/backlot (deleted)");
        assert_eq!(
            resolve_live_exe(Some(current), |q| q == current),
            PathBuf::from("/home/user/bin/backlot (deleted)")
        );
    }
}

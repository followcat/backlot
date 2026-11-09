//! The stage's startup path handling: a state base reached through
//! a symlink resolves to the real path the OS reports to terminals
//! (`term.cwd`) and git (`worktree list`), and a base that cannot
//! resolve keeps the path as given.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::daemon::resolved_base;

static NEXT: AtomicU64 = AtomicU64::new(0);

fn tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "backlot-daemon-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A symlinked base is created first when missing, then resolved to
/// the same path the OS reports — so every path downstream compares
/// equal to a terminal's cwd and a worktree listing.
#[test]
#[cfg(unix)]
fn state_base_resolves_through_a_symlink() {
    let root = tmp();
    // Missing first: created, then resolved.
    let fresh = root.join("fresh");
    assert_eq!(resolved_base(fresh.clone()), fresh.canonicalize().unwrap());
    assert!(fresh.is_dir());
    // Reached through a symlink — macOS's /var tmp: the real path.
    let real = root.join("real");
    fs::create_dir_all(&real).unwrap();
    let link = root.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let resolved = resolved_base(link.clone());
    assert_eq!(resolved, real.canonicalize().unwrap());
    // A base that cannot resolve keeps the path as given.
    let dangling = root.join("dangling");
    std::os::unix::fs::symlink(root.join("gone"), &dangling).unwrap();
    assert_eq!(resolved_base(dangling.clone()), dangling);
}

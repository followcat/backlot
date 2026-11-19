//! Test-only scratch directories that clean up after themselves.

#![allow(dead_code)]

use std::fs;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A unique directory under the system temp dir, removed when dropped —
/// including while a panicking test unwinds.
pub struct TestDir {
    path: PathBuf,
    /// Set by [`keep`](Self::keep) — a teardown that must not delete its
    /// evidence.
    keep: bool,
}

impl TestDir {
    /// An empty directory named `<prefix>-<pid>-<n>` under
    /// `std::env::temp_dir()`.
    pub fn new(prefix: &str) -> TestDir {
        Self::new_in(&std::env::temp_dir(), prefix)
    }

    /// [`new`](Self::new) under `parent`, for tests that need a short path.
    pub fn new_in(parent: &Path, prefix: &str) -> TestDir {
        loop {
            let path = parent.join(format!(
                "{prefix}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            // Claim the name atomically; never adopt a leftover directory.
            match fs::create_dir(&path) {
                Ok(()) => {
                    return TestDir { path, keep: false };
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("create test dir {}: {e}", path.display()),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Keeps the directory when it drops.
    pub fn keep(&mut self) {
        self.keep = true;
    }
}

impl Deref for TestDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for TestDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        if std::env::var_os("HC_TUI_KEEP_TEST_DIRS").is_some() {
            eprintln!("keeping test dir {}", self.path.display());
            return;
        }
        let _ = fs::remove_dir_all(&self.path);
    }
}

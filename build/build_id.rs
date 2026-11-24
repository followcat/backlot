// Shared build-script body: emits `BACKLOT_BUILD_ID` for the crates
// whose code reads it — `hc-tui`'s version line and update hint, and
// `backlot-stage`'s published `backlot_build`. Each crate's `build.rs`
// includes this file.
//
// The id is `git rev-parse --short=12 HEAD`, with `-dirty` appended
// while the tracked tree has changes, so two builds of one version are
// still distinguishable (S2). A build with no git or no repository —
// crates.io, a source tarball — emits nothing and the version line
// stays bare.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn main() {
    let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") else {
        return;
    };
    let root = PathBuf::from(manifest);
    let Some(head) = git(&root, &["rev-parse", "--short=12", "HEAD"]) else {
        return;
    };
    let head = head.trim();
    if head.is_empty() {
        return;
    }
    // `--ignore-submodules=all` keeps a vendored checkout's own link
    // from deciding this build's identity; the index and tracked files
    // still count.
    let dirty = git(
        &root,
        &[
            "status",
            "--porcelain",
            "--untracked-files=no",
            "--ignore-submodules=all",
        ],
    )
    .is_some_and(|out| !out.trim().is_empty());
    let build = if dirty {
        format!("{head}-dirty")
    } else {
        head.to_owned()
    };
    println!("cargo:rustc-env=BACKLOT_BUILD_ID={build}");
    // Rerun when HEAD moves: the HEAD file and the ref it names.
    // `--git-path` resolves both in a linked worktree, where `.git` is
    // a file.
    if let Some(path) = git(&root, &["rev-parse", "--git-path", "HEAD"]) {
        rerun(&root, &path);
    }
    if let Some(reference) = git(&root, &["symbolic-ref", "-q", "HEAD"])
        && let Some(path) = git(&root, &["rev-parse", "--git-path", reference.trim()])
    {
        rerun(&root, &path);
    }
    // The shared body itself, since Cargo tracks only each crate's
    // `build.rs`, not the file it includes.
    println!(
        "cargo:rerun-if-changed={}",
        root.join("../../build/build_id.rs").display()
    );
}

/// One git query from `root`; `None` when git or the repository is
/// absent or the command fails.
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `cargo:rerun-if-changed` needs a path; `--git-path` may print one
/// relative to the repository root.
fn rerun(root: &Path, path: &str) {
    let path = path.trim();
    if path.is_empty() {
        return;
    }
    let path = Path::new(path);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    println!("cargo:rerun-if-changed={}", absolute.display());
}

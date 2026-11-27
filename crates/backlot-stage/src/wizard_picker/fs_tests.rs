//! The real seams against a real filesystem — a scratch tree and
//! real `git`: the `Repo` probe marks repositories and leaves
//! children and plain dirs alone, `list_dirs` reads directories and
//! reports `(no access)` as `None`.

use std::fs;

use super::fs::{list_dirs, probe};
use super::tree::Repo;
use crate::cli_testkit::{git, scratch};

#[test]
fn the_probe_marks_repositories_only() {
    let (scratch, _env) = scratch();
    let repo = scratch.files.join("acme-shop");
    fs::create_dir_all(repo.join("src")).unwrap();
    git(&repo, &["init", "-b", "main"]);
    // No commit yet: Empty.
    assert_eq!(probe(&repo), Repo::Empty);
    fs::write(repo.join("f"), "x").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);
    match probe(&repo) {
        Repo::Commit { branch, .. } => assert_eq!(branch, "main"),
        other => panic!("expected Commit, got {other:?}"),
    }
    // A child inside the work tree is a plain directory.
    assert_eq!(probe(&repo.join("src")), Repo::Dir);
    // The scratch dir holds the repository — a workspace.
    assert_eq!(probe(&scratch.files), Repo::Workspace { repos: 1 });
    // A path that is no directory is missing.
    assert_eq!(probe(&scratch.files.join("gone")), Repo::Missing);
}

#[test]
fn list_dirs_lists_only_directories() {
    let (scratch, _env) = scratch();
    let dir = &scratch.files;
    fs::create_dir_all(dir.join("a")).unwrap();
    fs::create_dir_all(dir.join("b")).unwrap();
    fs::write(dir.join("file.txt"), "x").unwrap();
    let mut got: Vec<String> = list_dirs(dir)
        .unwrap()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    got.sort();
    assert_eq!(got, vec!["a", "b"]);
    // Missing dir: None — the picker shows (no access).
    assert!(list_dirs(&dir.join("nope")).is_none());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let closed = dir.join("closed");
        fs::create_dir_all(&closed).unwrap();
        fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).unwrap();
        assert!(list_dirs(&closed).is_none());
        fs::set_permissions(&closed, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// The same weak-mark rule as `paths::is_git`: a `.git` directory
/// git refuses is not a repository — the shape answers as if the
/// mark were absent, and a broken child does not count toward a
/// parent's workspace.
#[test]
fn a_broken_git_mark_is_no_repository() {
    let (scratch, _env) = scratch();
    let broken = scratch.files.join("orchard");
    fs::create_dir_all(broken.join(".git")).unwrap();
    assert_eq!(probe(&broken), Repo::Dir);
    assert_eq!(probe(&scratch.files), Repo::Dir);
    let repo = scratch.files.join("acme-shop");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    assert_eq!(probe(&scratch.files), Repo::Workspace { repos: 1 });
}

/// The picker's workspace count is distinct repositories, the same
/// list `paths::member_repos` hands the launch: linked worktrees of
/// one repository collapse to a single member (D4).
#[test]
fn linked_worktrees_count_as_one_workspace_member() {
    let (scratch, _env) = scratch();
    let ws = scratch.files.join("mono");
    let core = ws.join("core");
    fs::create_dir_all(&core).unwrap();
    git(&core, &["init", "-b", "main"]);
    fs::write(core.join("f"), "x").unwrap();
    git(&core, &["add", "."]);
    git(&core, &["commit", "-m", "base"]);
    git(
        &core,
        &[
            "worktree",
            "add",
            "-b",
            "wt",
            &ws.join("core-verify").to_string_lossy(),
        ],
    );
    let web = ws.join("web");
    fs::create_dir_all(&web).unwrap();
    git(&web, &["init", "-b", "main"]);
    assert_eq!(probe(&ws), Repo::Workspace { repos: 2 });
}

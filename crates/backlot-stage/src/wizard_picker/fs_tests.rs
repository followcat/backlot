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
    // A child inside the work tree is no repository.
    assert_eq!(probe(&repo.join("src")), Repo::None);
    // A plain directory is no repository either.
    assert_eq!(probe(&scratch.files), Repo::None);
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

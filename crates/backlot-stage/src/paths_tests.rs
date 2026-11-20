//! `paths` tests (D4): path-shape classification, the `--init` /
//! `--no-git` flag rules, and the launch-time git initialization —
//! all in scratch trees with real git.

use std::fs;
use std::path::Path;

use crate::cli_testkit::*;
use crate::paths::{self, PathShape};

/// A git repo with one commit, or with none when `commit` is false.
fn repo(path: &Path, commit: bool) {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "-b", "main"]);
    if commit {
        fs::write(path.join("f"), "base\n").unwrap();
        git(path, &["add", "."]);
        git(path, &["commit", "-m", "base"]);
    }
}

#[test]
fn classify_tells_repo_workspace_and_plain() {
    let (scratch, _env) = scratch();
    let files = &scratch.files;

    let ready = files.join("acme-shop");
    repo(&ready, true);
    assert_eq!(
        paths::classify(&ready),
        PathShape::Repo { has_commits: true }
    );

    let empty = files.join("fresh");
    repo(&empty, false);
    assert_eq!(
        paths::classify(&empty),
        PathShape::Repo { has_commits: false }
    );

    // A directory of sibling repositories is a workspace — direct
    // children only, sorted; a deeper repo never counts.
    let ws = files.join("mono");
    repo(&ws.join("beta-api"), true);
    repo(&ws.join("web"), true);
    fs::create_dir_all(ws.join("docs")).unwrap();
    repo(&ws.join("docs").join("inner"), true);
    let PathShape::Workspace { members } = paths::classify(&ws) else {
        panic!("expected a workspace");
    };
    let names: Vec<String> = members
        .iter()
        .map(|m| {
            Path::new(m)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, ["beta-api", "web"]);

    let plain = files.join("notes");
    fs::create_dir_all(&plain).unwrap();
    assert_eq!(paths::classify(&plain), PathShape::Plain);
}

#[test]
fn the_flag_rules_follow_the_shape() {
    let (scratch, _env) = scratch();
    let files = &scratch.files;

    let ready = files.join("acme-shop");
    repo(&ready, true);
    let ready_s = ready.to_string_lossy().into_owned();
    assert!(paths::check(&ready_s, false, false).is_ok());
    assert!(paths::check(&ready_s, true, false).is_err()); // --init has no effect
    let msg = paths::check(&ready_s, false, true).unwrap_err();
    assert!(format!("{msg:?}").contains("--no-git"), "{msg:?}");

    let empty = files.join("fresh");
    repo(&empty, false);
    let empty_s = empty.to_string_lossy().into_owned();
    assert!(paths::check(&empty_s, false, false).is_ok());
    assert!(paths::check(&empty_s, true, false).is_ok()); // tolerated: it asks for what happens

    let ws = files.join("mono");
    repo(&ws.join("beta-api"), true);
    let ws_s = ws.to_string_lossy().into_owned();
    let binding = paths::check(&ws_s, false, false).unwrap();
    assert_eq!(binding.members.len(), 1);
    assert!(!binding.no_git);
    assert!(paths::check(&ws_s, true, false).is_err());
    assert!(paths::check(&ws_s, false, true).is_err());

    let plain = files.join("notes");
    fs::create_dir_all(&plain).unwrap();
    let plain_s = plain.to_string_lossy().into_owned();
    let msg = paths::check(&plain_s, false, false).unwrap_err();
    assert!(format!("{msg:?}").contains("--init"), "{msg:?}");
    assert!(format!("{msg:?}").contains("--no-git"), "{msg:?}");
    assert!(paths::check(&plain_s, false, true).unwrap().no_git);
    assert!(paths::check(&plain_s, true, false).is_ok());
}

/// `init` + `initialize` build a repository where a plain directory
/// stood; `no_git` and a bare `initialize` leave the disk alone.
#[test]
fn initialize_makes_the_repo_the_plan_promised() {
    let (scratch, _env) = scratch();
    let plain = scratch.files.join("notes");
    fs::create_dir_all(&plain).unwrap();
    let argv = paths::init_args(&plain);
    assert_eq!(argv[0], vec!["init", &plain.to_string_lossy()]);
    assert!(argv[1].iter().any(|a| a == "--allow-empty"));
    assert!(argv[1].iter().any(|a| a == "chore: initial commit"));

    paths::initialize(&plain.to_string_lossy(), &[], false, true).unwrap();
    assert!(plain.join(".git").exists());
    assert_eq!(
        git(&plain, &["log", "-1", "--format=%s"]),
        "chore: initial commit"
    );

    // A repository with no commits gets the same empty commit — a
    // bound repo or each workspace member alike.
    let ws = scratch.files.join("mono");
    repo(&ws.join("beta-api"), false);
    repo(&ws.join("web"), false);
    let members = paths::member_repos(&ws);
    paths::initialize(&ws.to_string_lossy(), &members, false, false).unwrap();
    for m in &members {
        assert_eq!(
            git(Path::new(m), &["log", "-1", "--format=%s"]),
            "chore: initial commit"
        );
    }

    // no_git touches nothing.
    let bare = scratch.files.join("bare");
    fs::create_dir_all(&bare).unwrap();
    paths::initialize(&bare.to_string_lossy(), &[], true, false).unwrap();
    assert!(!bare.join(".git").exists());
}

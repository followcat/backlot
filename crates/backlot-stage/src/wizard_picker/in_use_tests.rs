//! The "repositories in use" group over fake terminal cwds: each
//! resolves up to its enclosing repository root — the nearest one,
//! a nested repository wins over its parent — deduplicated in
//! first-seen order, capped by the picker itself.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::in_use::{collect, repo_root};

/// `is_repo` answered by a set of repository roots.
fn repos<'a>(roots: &'a BTreeSet<PathBuf>) -> Box<dyn Fn(&Path) -> bool + 'a> {
    Box::new(move |p| roots.contains(p))
}

/// The branch seam answered by a table: path → branch.
fn branches(table: Vec<(PathBuf, &'static str)>) -> impl Fn(&Path) -> Option<String> {
    move |p| {
        table
            .iter()
            .find(|(path, _)| path == p)
            .map(|(_, branch)| (*branch).to_string())
    }
}

/// U7 fix 2: a studio's role terminals work inside backlot's own state
/// dir, on branches in backlot's `backlot/<studio>/…` namespace. Nobody
/// starts a studio from them, so they stay out of the group; the
/// repositories those studios were created from stay in.
#[test]
fn backlot_s_own_worktrees_stay_out_of_the_in_use_group() {
    let state = PathBuf::from("/state");
    let mut roots = BTreeSet::new();
    // A role's worktree inside the state dir — the case the state-dir
    // rule catches.
    roots.insert(PathBuf::from("/state/studios/acme-shop/work/t1"));
    // A checkout on a `backlot/` branch outside the state dir — the
    // case the branch rule catches, which a remote core needs.
    roots.insert(PathBuf::from("/home/user/other-shop"));
    // A plain repository: the one row that belongs.
    roots.insert(PathBuf::from("/home/user/acme-shop"));
    let is_repo = repos(&roots);
    let branch = branches(vec![
        (
            PathBuf::from("/home/user/other-shop"),
            "backlot/other-shop/integration",
        ),
        (PathBuf::from("/home/user/acme-shop"), "main"),
    ]);
    let cwds = vec![
        PathBuf::from("/state/studios/acme-shop/work/t1/src"),
        PathBuf::from("/home/user/other-shop"),
        PathBuf::from("/home/user/acme-shop/src"),
    ];
    assert_eq!(
        collect(cwds, &state, &*is_repo, &branch),
        vec![PathBuf::from("/home/user/acme-shop")]
    );
}

/// The namespace is `backlot/…`: a branch that merely starts with those
/// letters, or is named `backlot` alone, is somebody else's and stays.
#[test]
fn only_the_backlot_namespace_is_left_out() {
    let state = PathBuf::from("/state");
    let mut roots = BTreeSet::new();
    roots.insert(PathBuf::from("/home/user/backlot-tools"));
    roots.insert(PathBuf::from("/home/user/backlot"));
    let is_repo = repos(&roots);
    let branch = branches(vec![
        (
            PathBuf::from("/home/user/backlot-tools"),
            "backlot-tools/wip",
        ),
        (PathBuf::from("/home/user/backlot"), "backlot"),
    ]);
    assert_eq!(
        collect(
            vec![
                PathBuf::from("/home/user/backlot-tools"),
                PathBuf::from("/home/user/backlot"),
            ],
            &state,
            &*is_repo,
            &branch,
        ),
        vec![
            PathBuf::from("/home/user/backlot-tools"),
            PathBuf::from("/home/user/backlot"),
        ]
    );
}

#[test]
fn a_cwd_resolves_to_its_nearest_repository_ancestor() {
    let mut roots = BTreeSet::new();
    roots.insert(PathBuf::from("/home/user/acme-shop"));
    let is_repo = repos(&roots);
    assert_eq!(
        repo_root(Path::new("/home/user/acme-shop/src/deep"), &*is_repo),
        Some(PathBuf::from("/home/user/acme-shop"))
    );
    // The repository itself is its own root.
    assert_eq!(
        repo_root(Path::new("/home/user/acme-shop"), &*is_repo),
        Some(PathBuf::from("/home/user/acme-shop"))
    );
    // No repository above: None.
    assert_eq!(
        repo_root(Path::new("/home/user/plain/sub"), &*is_repo),
        None
    );
}

#[test]
fn a_nested_repository_wins_over_its_parent() {
    let mut roots = BTreeSet::new();
    roots.insert(PathBuf::from("/home/user"));
    roots.insert(PathBuf::from("/home/user/acme-shop"));
    let is_repo = repos(&roots);
    assert_eq!(
        repo_root(Path::new("/home/user/acme-shop/src"), &*is_repo),
        Some(PathBuf::from("/home/user/acme-shop"))
    );
}

#[test]
fn cwds_deduplicate_to_roots_in_first_seen_order() {
    let mut roots = BTreeSet::new();
    roots.insert(PathBuf::from("/home/user/acme-shop"));
    roots.insert(PathBuf::from("/home/user/code/web-app"));
    let is_repo = repos(&roots);
    let no_branch = |_: &Path| None;
    let cwds = vec![
        PathBuf::from("/home/user/acme-shop/src"),
        PathBuf::from("/home/user/acme-shop/tests"), // same root
        PathBuf::from("/home/user/code/web-app"),
        PathBuf::from("/opt/nowhere"), // no repository
        PathBuf::from(""),             // no cwd at all
    ];
    assert_eq!(
        collect(cwds, Path::new("/state"), &*is_repo, &no_branch),
        vec![
            PathBuf::from("/home/user/acme-shop"),
            PathBuf::from("/home/user/code/web-app"),
        ]
    );
}

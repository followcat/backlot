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
    let cwds = vec![
        PathBuf::from("/home/user/acme-shop/src"),
        PathBuf::from("/home/user/acme-shop/tests"), // same root
        PathBuf::from("/home/user/code/web-app"),
        PathBuf::from("/opt/nowhere"), // no repository
        PathBuf::from(""),             // no cwd at all
    ];
    assert_eq!(
        collect(cwds, &*is_repo),
        vec![
            PathBuf::from("/home/user/acme-shop"),
            PathBuf::from("/home/user/code/web-app"),
        ]
    );
}

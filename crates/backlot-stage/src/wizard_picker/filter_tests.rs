//! The picker's path rules: `~` expansion, the deepest existing
//! directory as the tree's root, the leftover first component as the
//! filter, substring before subsequence, repositories ranked first,
//! and the longest common prefix Tab completes.

use std::path::{Path, PathBuf};

use super::filter::*;

fn home() -> PathBuf {
    PathBuf::from("/home/user")
}

fn names(paths: &[PathBuf]) -> Vec<&str> {
    paths
        .iter()
        .map(|p| p.file_name().unwrap().to_str().unwrap())
        .collect()
}

/// `~` becomes home, a relative path resolves under home, and an
/// absolute one passes through.
#[test]
fn tilde_and_relative_expand() {
    assert_eq!(expand("~", &home()), home());
    assert_eq!(expand("~/x", &home()), home().join("x"));
    assert_eq!(expand("x", &home()), home().join("x"));
    assert_eq!(expand("", &home()), home());
    assert_eq!(expand("/abs/x", &home()), PathBuf::from("/abs/x"));
}

/// The home prefix spells as `~`; other paths pass through.
#[test]
fn the_home_prefix_shortens() {
    assert_eq!(shorten(&home(), &home()), "~");
    assert_eq!(shorten(&home().join("x/y"), &home()), "~/x/y");
    assert_eq!(shorten(Path::new("/other"), &home()), "/other");
    // A look-alike prefix is not home: /home/user2 is not ~/…
    assert_eq!(shorten(Path::new("/home/user2"), &home()), "/home/user2");
}

/// The root is the deepest existing directory of the typed path;
/// the first leftover component is the filter.
#[test]
fn the_root_is_the_deepest_existing_directory() {
    // /x exists, /x/y exists, /x/y/ba does not.
    let dirs: &[&str] = &["/x", "/x/y"];
    let is_dir = |p: &Path| dirs.contains(&p.to_str().unwrap());
    let (root, filter) = split(Path::new("/x/y/ba"), &is_dir);
    assert_eq!(root, PathBuf::from("/x/y"));
    assert_eq!(filter, "ba");
    // The whole typed path exists: it is the root, nothing filters.
    let (root, filter) = split(Path::new("/x/y"), &is_dir);
    assert_eq!(root, PathBuf::from("/x/y"));
    assert_eq!(filter, "");
    // Two leftover components: only the first filters.
    let (root, filter) = split(Path::new("/x/ba/more"), &is_dir);
    assert_eq!(root, PathBuf::from("/x"));
    assert_eq!(filter, "ba");
}

/// Substring hits beat subsequence hits; neither matches nothing.
#[test]
fn substring_before_subsequence() {
    assert_eq!(matches("bac", "backlot"), Some(Match::Substr));
    assert_eq!(matches("ACK", "backlot"), Some(Match::Substr)); // case-insensitive
    assert_eq!(matches("bcl", "backlot"), Some(Match::Subseq));
    assert_eq!(matches("xyz", "backlot"), None);
    assert_eq!(matches("", "backlot"), Some(Match::Substr));
    // A case-insensitive subsequence also matches.
    assert_eq!(matches("BCL", "backlot"), Some(Match::Subseq));
}

/// Filtered children rank by match kind, then repositories first,
/// then name.
#[test]
fn ranking_is_match_repo_name() {
    let kids = vec![
        PathBuf::from("/r/zeta"),  // subseq of "za", plain
        PathBuf::from("/r/zebra"), // subseq of "za", a repo
        PathBuf::from("/r/zapp"),  // substring of "za", plain
        PathBuf::from("/r/bizar"), // substring of "za", a repo
        PathBuf::from("/r/omega"), // no match
    ];
    let repos: &[&str] = &["/r/zebra", "/r/bizar"];
    let is_repo = |p: &Path| repos.contains(&p.to_str().unwrap());
    let ranked = rank(kids, "za", &is_repo);
    let got = names(&ranked);
    // Substring hits first (repo bizar before plain zapp), then
    // subsequence hits (repo zebra before plain zeta).
    assert_eq!(got, vec!["bizar", "zapp", "zebra", "zeta"]);
}

/// A `.` filter reveals hidden names; without it they, `node_modules`,
/// `target` and `.git` are always left out.
#[test]
fn hidden_names_answer_a_dot_filter_only() {
    let kids = vec![
        PathBuf::from("/r/.secret"),
        PathBuf::from("/r/.git"),
        PathBuf::from("/r/node_modules"),
        PathBuf::from("/r/target"),
        PathBuf::from("/r/seed"),
    ];
    let no_repo = |_: &Path| false;
    let plain = rank(kids.clone(), "", &no_repo);
    assert_eq!(names(&plain), vec!["seed"]);
    // The filter starts with `.` — hidden names answer, the skip
    // list still does not.
    let dotted = rank(kids, ".", &no_repo);
    assert_eq!(names(&dotted), vec![".secret"]);
    assert!(wanted(".secret", true));
    assert!(!wanted(".secret", false));
    assert!(!wanted(".git", true));
    assert!(!wanted("node_modules", false));
    assert!(!wanted("target", false));
}

/// Tab completes the longest common prefix of the match names.
#[test]
fn the_longest_common_prefix() {
    assert_eq!(lcp(["backlot", "backplane", "backup"].into_iter()), "back");
    assert_eq!(lcp(["backlot"].into_iter()), "backlot");
    assert_eq!(lcp([].into_iter()), "");
    assert_eq!(lcp(["a", "b"].into_iter()), "");
}

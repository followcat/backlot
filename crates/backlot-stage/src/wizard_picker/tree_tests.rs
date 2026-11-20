//! Row bookkeeping: the 500-child cap line, the tree connectors a
//! row carries, and which rows may hold the selection.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::state::{Key, Picker};
use super::tree::{CAP, Lister, Repo, Row, Seams, age_text};

/// A single directory `dir` holding `n` plain children — enough to
/// test the cap without spelling 600 names.
fn big_fs(n: usize) -> impl Fn(&Path) -> Option<Vec<PathBuf>> {
    let dir = PathBuf::from("/big");
    move |p| {
        if p == dir {
            Some((0..n).map(|i| dir.join(format!("d{i:03}"))).collect())
        } else {
            Some(vec![])
        }
    }
}

fn seams<'a>(lister: Lister<'a>) -> Seams<'a> {
    Seams {
        lister,
        repo: Box::new(|_| Repo::Dir),
        is_dir: Box::new(|p| p == Path::new("/big") || p.starts_with("/big")),
        is_repo: Box::new(|_| false),
    }
}

/// More than 500 children shows the first 500 plus one line that
/// says how many were left out — and it never panics.
#[test]
fn the_five_hundred_child_cap() {
    let lister = big_fs(600);
    let s = seams(Box::new(lister));
    let mut p = Picker::new(s, PathBuf::from("/home/user"), Path::new("/big"), vec![]);
    // "/big" is no repository, so the input starts at ~; set it.
    p.set_input("/big/");
    let dirs = p
        .rows()
        .iter()
        .filter(|r| matches!(r, Row::Dir { .. }))
        .count();
    assert_eq!(dirs, CAP, "expected {CAP} directory rows");
    let cap = p.rows().iter().find_map(|r| match r {
        Row::Cap { left, .. } => Some(*left),
        _ => None,
    });
    assert_eq!(cap, Some(100));
    // The cap row never takes the selection.
    assert!(!p.rows().last().unwrap().selectable());
    p.key(Key::End);
    let last = p.rows().len() - 1;
    assert_ne!(p.sel(), last, "selection skips the cap line");
}

/// `is_expanded` tracks the in-place opens; `age_text` is the
/// commit age next to the branch.
#[test]
fn expansion_and_age_helpers() {
    // `/p/a` has one child so Right expands it instead of entering.
    let kids: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::from([
        (PathBuf::from("/p"), vec![PathBuf::from("/p/a")]),
        (PathBuf::from("/p/a"), vec![PathBuf::from("/p/a/x")]),
    ]);
    let lister = move |p: &Path| kids.get(p).cloned().or(Some(vec![]));
    let s = Seams {
        lister: Box::new(lister),
        repo: Box::new(|_| Repo::Dir),
        is_dir: Box::new(|p| p == Path::new("/p") || p.starts_with("/p")),
        is_repo: Box::new(|_| false),
    };
    let mut p = Picker::new(s, PathBuf::from("/home/user"), Path::new("/p"), vec![]);
    p.set_input("/p/");
    assert!(!p.is_expanded(Path::new("/p/a")));
    p.key(Key::Up);
    p.key(Key::Right);
    assert!(p.is_expanded(Path::new("/p/a")));
    assert_eq!(age_text(30), "just now");
    assert_eq!(age_text(300), "5 min ago");
    assert_eq!(age_text(7200), "2 h ago");
    assert_eq!(age_text(86400), "1 d ago");
}

/// Expanded children carry their connector ancestry: each depth's
/// `last` flag drives ├/└ and the | continuations.
#[test]
fn connector_ancestry_is_kept() {
    // /p holds [a, z]; a holds [x].
    let p_root = PathBuf::from("/p");
    let kids: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::from([
        (p_root.clone(), vec![p_root.join("a"), p_root.join("z")]),
        (p_root.join("a"), vec![p_root.join("a/x")]),
        (p_root.join("z"), vec![]),
    ]);
    let listed = BTreeSet::from([p_root.clone()]);
    let lister = |p: &Path| kids.get(p).cloned().or(Some(vec![]));
    let kids2 = kids.clone();
    let s = Seams {
        lister: Box::new(lister),
        repo: Box::new(|_| Repo::Dir),
        is_dir: Box::new(move |p| listed.contains(p) || kids2.contains_key(p)),
        is_repo: Box::new(|_| false),
    };
    let mut p = Picker::new(s, PathBuf::from("/home/user"), Path::new("/p"), vec![]);
    p.set_input("/p/");
    // Expand "a": its child x is its last (and only) child; "a" is
    // not the last of the root's children ("z" is). "a" is the
    // first row — Up takes tree focus, Right expands.
    assert!(p.rows()[0].path().unwrap().ends_with("a"));
    p.key(Key::Up);
    p.key(Key::Right);
    let x = p.rows().iter().find_map(|r| match r {
        Row::Dir { path, lasts } if path.ends_with("x") => Some(lasts.clone()),
        _ => None,
    });
    assert_eq!(x, Some(vec![false, true]));
}

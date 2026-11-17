//! The path half of the picker: `~` in the input line, the typed
//! text split into the deepest existing directory (the tree's root)
//! and the leftover first component (the filter), and the match
//! rules the children's ranking follows — case-insensitive
//! substring first, then subsequence, repositories before plain
//! directories, names last.

use std::path::{Path, PathBuf};

/// Names the tree never lists — a `.` filter reveals hidden
/// directories but never these.
const SKIP: &[&str] = &[".git", "node_modules", "target"];

/// How a name answers the filter — `Substr` ranks before `Subseq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Match {
    Substr,
    Subseq,
}

/// `~` becomes `home`; a relative text resolves under `home` (an
/// empty one is `home` itself) and an absolute one passes through.
pub fn expand(text: &str, home: &Path) -> PathBuf {
    if text == "~" {
        home.to_path_buf()
    } else if let Some(rest) = text.strip_prefix("~/") {
        home.join(rest)
    } else if Path::new(text).is_absolute() {
        PathBuf::from(text)
    } else {
        home.join(text)
    }
}

/// `abs` spelled with `~` for the home prefix; every other path
/// keeps its absolute form.
pub fn shorten(abs: &Path, home: &Path) -> String {
    match abs.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.to_string_lossy()),
        Err(_) => abs.to_string_lossy().into_owned(),
    }
}

/// Split `abs` into the deepest existing directory (the tree's
/// root) and the first leftover component (the filter). `is_dir`
/// answers the filesystem's side. A root of `/` is the last resort.
pub fn split(abs: &Path, is_dir: &dyn Fn(&Path) -> bool) -> (PathBuf, String) {
    let mut root = abs.to_path_buf();
    loop {
        if is_dir(&root) {
            break;
        }
        if !root.pop() {
            root = PathBuf::from("/");
            break;
        }
    }
    // `root` is an ancestor of `abs` — the leftovers are the
    // components after the root's own count (literal `..` segments
    // count too, which keeps the split working for odd paths).
    let left: Vec<_> = abs.components().skip(root.components().count()).collect();
    let filter = left
        .first()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_default();
    (root, filter)
}

/// The path below `root`'s first leftover component — the typed
/// suffix Tab keeps after rewriting the filter.
pub fn rest_after(abs: &Path, root: &Path) -> String {
    abs.components()
        .skip(root.components().count() + 1)
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// `filter` against `name`, case-insensitive: a substring hit, else
/// a subsequence hit, else none. The empty filter hits everything.
pub fn matches(filter: &str, name: &str) -> Option<Match> {
    if filter.is_empty() {
        return Some(Match::Substr);
    }
    let f = filter.to_lowercase();
    let n = name.to_lowercase();
    if n.contains(&f) {
        return Some(Match::Substr);
    }
    let mut rest = n.chars();
    if f.chars().all(|fc| rest.by_ref().any(|nc| nc == fc)) {
        return Some(Match::Subseq);
    }
    None
}

/// May `name` be listed? `SKIP` never is; a hidden name answers
/// only a filter that starts with `.`.
pub fn wanted(name: &str, show_hidden: bool) -> bool {
    if SKIP.contains(&name) {
        return false;
    }
    show_hidden || !name.starts_with('.')
}

/// A child path's file name — the text the filter and the tree use.
fn name(path: &Path) -> Option<String> {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
}

/// Filter and rank `kids`: [`wanted`] and [`matches`] decide, then
/// match kind, repository-first and name order the rows. `is_repo`
/// is the cheap `.git`-exists probe — the row's own `git log` is
/// only ever asked for visible rows, not for the ranking.
pub fn rank(kids: Vec<PathBuf>, filter: &str, is_repo: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let show_hidden = filter.starts_with('.');
    let mut scored: Vec<(Match, bool, String, PathBuf)> = kids
        .into_iter()
        .filter_map(|path| {
            let name = name(&path)?;
            if !wanted(&name, show_hidden) {
                return None;
            }
            let m = matches(filter, &name)?;
            Some((m, !is_repo(&path), name.to_lowercase(), path))
        })
        .collect();
    scored.sort();
    scored.into_iter().map(|(_, _, _, path)| path).collect()
}

/// The longest common prefix of the names — what Tab inserts.
pub fn lcp<'a>(mut names: impl Iterator<Item = &'a str>) -> String {
    let mut pre = match names.next() {
        Some(first) => first.to_string(),
        None => return String::new(),
    };
    for name in names {
        while !name.starts_with(pre.as_str()) {
            pre.pop();
        }
        if pre.is_empty() {
            break;
        }
    }
    pre
}

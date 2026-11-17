//! The tree half of the picker: what a row is, what a directory's
//! git probe says, and the seams the state machine asks the world
//! through. `state` builds the rows; `render` draws them; nothing
//! here lists a real directory or runs git — [`Seams`] inject that.

use std::path::{Path, PathBuf};

/// A directory listing shows at most this many children plus one
/// "left out" line — a huge folder never stalls the picker.
pub const CAP: usize = 500;

/// What `git log -1` says about a directory — the picker's cache
/// fills it once per asked path.
#[derive(Debug, Clone, PartialEq)]
pub enum Repo {
    /// A git repository with a commit: its branch and the last
    /// commit's age in seconds.
    Commit { branch: String, age_s: u64 },
    /// A git repository with no commit yet — it cannot be picked.
    Empty,
    /// Not a git repository.
    None,
}

impl Repo {
    /// A commit behind it — Enter may pick this row.
    pub fn pickable(&self) -> bool {
        matches!(self, Repo::Commit { .. })
    }
}

/// One drawn line in the picker's scroll area.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    /// A repository a live terminal already works in — the
    /// "repositories in use" group above the tree.
    Used { path: PathBuf },
    /// A directory row. `lasts[k]` is the is-last-child flag of the
    /// ancestor at depth `k`; `lasts[depth]` is the row's own — the
    /// renderer derives ├/└ and the │ continuations from it.
    Dir { path: PathBuf, lasts: Vec<bool> },
    /// "… n left out" under a capped listing — never selectable.
    Cap { depth: usize, left: usize },
}

impl Row {
    /// The selection may rest on repository and directory rows only.
    pub fn selectable(&self) -> bool {
        !matches!(self, Row::Cap { .. })
    }

    /// The path a row carries (`Cap` has none).
    pub fn path(&self) -> Option<&Path> {
        match self {
            Row::Used { path } | Row::Dir { path, .. } => Some(path),
            Row::Cap { .. } => None,
        }
    }
}

/// How a directory's children are listed — `None` when it cannot
/// be read.
pub type Lister<'a> = Box<dyn Fn(&Path) -> Option<Vec<PathBuf>> + 'a>;

/// Everything the picker asks the world: injected, so a test runs
/// the whole state machine over a fake tree.
pub struct Seams<'a> {
    /// The child directories of `path` — `None` when it cannot be
    /// read; the picker then shows `(no access)` and goes on.
    pub lister: Lister<'a>,
    /// `git log -1` as a [`Repo`]; the picker caches it per path.
    pub repo: Box<dyn Fn(&Path) -> Repo + 'a>,
    /// `path` exists and is a directory — the root's side of `split`.
    pub is_dir: Box<dyn Fn(&Path) -> bool + 'a>,
    /// The cheap `.git`-exists check the ranking uses — the one
    /// probe every candidate gets, because match order puts
    /// repositories first.
    pub is_repo: Box<dyn Fn(&Path) -> bool + 'a>,
}

/// `7200` seconds as `2 h ago` — the commit age a repository row
/// carries next to its branch.
pub fn age_text(age_s: u64) -> String {
    if age_s < 60 {
        "just now".to_string()
    } else if age_s < 3600 {
        format!("{} min ago", age_s / 60)
    } else if age_s < 86400 {
        format!("{} h ago", age_s / 3600)
    } else {
        format!("{} d ago", age_s / 86400)
    }
}

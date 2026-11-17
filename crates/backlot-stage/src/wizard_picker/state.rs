//! The picker's state machine: the input text and its cursor, the
//! root/filter split, the expanded-directory set, the flat row list
//! and the selection. `key` consumes a [`Key`]; [`Outcome`] is the
//! only way out — a picked repository or a cancel.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use super::filter;
use super::tree::{CAP, Repo, Row, Seams};

/// Where a key lands — the input line or the tree. Esc moves
/// tree→input; a second Esc cancels the wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Input,
    Tree,
}

/// A key, already mapped from the terminal's event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Backspace,
    Left,
    Right,
    Up,
    Down,
    PgUp,
    PgDn,
    Home,
    End,
    Tab,
    Enter,
    Esc,
    CtrlC,
}

/// How the picker ends: the repository path picked or a cancel —
/// nothing was ever written.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Picked(PathBuf),
    Cancelled,
}

/// Enter's answer for every directory it cannot take — a plain
/// directory, a repository without commits, an unreadable one.
const NOT_PICKABLE: &str = "not a git repository with a commit";

/// The "repositories in use" group shows at most this many.
const IN_USE_CAP: usize = 8;

pub struct Picker<'a> {
    seams: Seams<'a>,
    home: PathBuf,
    /// The input line's text and the cursor's byte offset.
    text: String,
    cursor: usize,
    focus: Focus,
    /// `true` once the input left its start — the "repositories in
    /// use" group only shows before that.
    dirty: bool,
    /// The deepest existing directory of `text`, and the leftover
    /// first component that filters its children.
    root: PathBuf,
    filter: String,
    /// `false` when the root's own listing failed — the tree area
    /// says `(no access)` then.
    root_listed: bool,
    /// The root's children, filtered and ranked — the tree's top
    /// level.
    matches: Vec<PathBuf>,
    /// Directories expanded in place.
    expanded: BTreeSet<PathBuf>,
    /// Listing cache — `None` answers `(no access)`.
    children: HashMap<PathBuf, Option<Vec<PathBuf>>>,
    /// The `git log -1` cache — one probe per asked path.
    repos: HashMap<PathBuf, Repo>,
    /// The flat scroll area (used group first, then the tree).
    rows: Vec<Row>,
    /// The selection's index into `rows`.
    sel: usize,
    in_use: Vec<PathBuf>,
    status: Option<String>,
    /// The scroll area's height, set by the renderer so PgUp/PgDn
    /// move by a page.
    page: usize,
}

impl<'a> Picker<'a> {
    /// `start` is the process's working directory: a repository with
    /// a commit fills the input, anything else starts at `~`.
    /// `in_use` is the "repositories in use" group (capped at 8).
    pub fn new(seams: Seams<'a>, home: PathBuf, start: &Path, in_use: Vec<PathBuf>) -> Picker<'a> {
        let mut p = Picker {
            seams,
            home,
            text: String::new(),
            cursor: 0,
            focus: Focus::Input,
            dirty: false,
            root: PathBuf::new(),
            filter: String::new(),
            root_listed: true,
            matches: Vec::new(),
            expanded: BTreeSet::new(),
            children: HashMap::new(),
            repos: HashMap::new(),
            rows: Vec::new(),
            sel: 0,
            in_use: in_use.into_iter().take(IN_USE_CAP).collect(),
            status: None,
            page: 8,
        };
        p.text = match p.repo(start) {
            Repo::Commit { .. } => filter::shorten(start, &p.home),
            _ => "~".to_string(),
        };
        p.cursor = p.text.len();
        p.reparse();
        p.sel = p.first_selectable();
        p
    }

    /// One key. `Some` is the picker asking to be dropped.
    pub fn key(&mut self, k: Key) -> Option<Outcome> {
        match k {
            Key::CtrlC => return Some(Outcome::Cancelled),
            Key::Esc => {
                if self.focus == Focus::Tree {
                    self.focus = Focus::Input;
                } else {
                    return Some(Outcome::Cancelled);
                }
            }
            Key::Char(c) => {
                self.text.insert(self.cursor, c);
                self.cursor += c.len_utf8();
                self.edited();
            }
            Key::Backspace => {
                if let Some(prev) = self.text[..self.cursor]
                    .char_indices()
                    .last()
                    .map(|(i, _)| i)
                {
                    self.text.drain(prev..self.cursor);
                    self.cursor = prev;
                    self.edited();
                }
            }
            Key::Left if self.focus == Focus::Input => {
                self.cursor = self.text[..self.cursor]
                    .char_indices()
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
            }
            Key::Right if self.focus == Focus::Input => {
                if let Some(c) = self.text[self.cursor..].chars().next() {
                    self.cursor += c.len_utf8();
                }
            }
            Key::Left => self.collapse_or_parent(),
            Key::Right => self.expand_or_enter(),
            Key::Up => {
                self.focus = Focus::Tree;
                self.move_sel(-1);
            }
            Key::Down => {
                self.focus = Focus::Tree;
                self.move_sel(1);
            }
            Key::PgUp => {
                self.focus = Focus::Tree;
                self.move_sel(-(self.page as isize));
            }
            Key::PgDn => {
                self.focus = Focus::Tree;
                self.move_sel(self.page as isize);
            }
            Key::Home => {
                self.focus = Focus::Tree;
                self.sel = self.first_selectable();
            }
            Key::End => {
                self.focus = Focus::Tree;
                self.sel = self.last_selectable();
            }
            Key::Tab => self.complete(),
            Key::Enter => return self.enter(),
        }
        None
    }

    // ---- what the renderer reads -------------------------------

    pub fn input(&self) -> &str {
        &self.text
    }
    /// The text cursor's byte offset.
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn focus(&self) -> Focus {
        self.focus
    }
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }
    pub fn sel(&self) -> usize {
        self.sel
    }
    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }
    /// The tree's root, `~`-spelled for the heading.
    pub fn root_display(&self) -> String {
        filter::shorten(&self.root, &self.home)
    }
    /// `false` — the root's own listing failed; the tree says
    /// `(no access)`.
    pub fn root_listed(&self) -> bool {
        self.root_listed
    }
    /// Leading `Used` rows — the group is empty once typing starts.
    pub fn in_use_rows(&self) -> usize {
        self.rows
            .iter()
            .take_while(|r| matches!(r, Row::Used { .. }))
            .count()
    }
    /// The scroll area's height — PgUp/PgDn's step.
    pub fn set_page(&mut self, page: usize) {
        self.page = page.max(1);
    }
    /// `path`'s cached `git log -1` answer — the renderer asks this
    /// once per visible row, so probes stay per-visible-row.
    pub fn repo(&mut self, path: &Path) -> Repo {
        self.repos
            .entry(path.to_path_buf())
            .or_insert_with(|| (self.seams.repo)(path))
            .clone()
    }
    /// `path`'s cached child directories — `None` is `(no access)`.
    pub fn kids(&mut self, path: &Path) -> Option<Vec<PathBuf>> {
        self.children
            .entry(path.to_path_buf())
            .or_insert_with(|| (self.seams.lister)(path))
            .clone()
    }
    /// `path` is expanded in place.
    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }
    /// The input set outright — tests and the start rule use it.
    pub fn set_input(&mut self, text: &str) {
        self.text = text.to_string();
        self.cursor = self.text.len();
        self.edited();
    }
    /// The tree's root — tests use it.
    #[cfg(test)]
    pub fn root(&self) -> &Path {
        &self.root
    }

    // ---- internals ----------------------------------------------

    /// A text change: reparse, hide the in-use group, refocus the
    /// input and take the selection to the first row.
    fn edited(&mut self) {
        self.dirty = true;
        self.focus = Focus::Input;
        self.status = None;
        self.reparse();
        self.sel = self.first_selectable();
    }

    /// Text → (root, filter, ranked matches) → rows.
    fn reparse(&mut self) {
        let abs = filter::expand(&self.text, &self.home);
        let (root, f) = filter::split(&abs, &*self.seams.is_dir);
        self.root = root;
        self.filter = f;
        let root = self.root.clone();
        self.matches = match self.kids(&root) {
            None => {
                self.root_listed = false;
                Vec::new()
            }
            Some(kids) => {
                self.root_listed = true;
                filter::rank(kids, &self.filter, &*self.seams.is_repo)
            }
        };
        self.rebuild_rows();
    }

    /// `rows` from `in_use` (while the input is untouched) plus the
    /// expanded tree under `matches`, capped.
    fn rebuild_rows(&mut self) {
        self.rows.clear();
        if !self.dirty {
            for path in self.in_use.clone() {
                self.rows.push(Row::Used { path });
            }
        }
        let shown = self.matches.iter().take(CAP).cloned().collect::<Vec<_>>();
        let n = shown.len();
        for (i, child) in shown.into_iter().enumerate() {
            self.push_dir(child, vec![i + 1 == n]);
        }
        if self.matches.len() > CAP {
            self.rows.push(Row::Cap {
                depth: 0,
                left: self.matches.len() - CAP,
            });
        }
        if self.rows.get(self.sel).is_none_or(|r| !r.selectable()) {
            self.sel = self.first_selectable();
        }
    }

    /// One directory row plus its expanded subtree — depth-first so
    /// children follow their parent.
    fn push_dir(&mut self, path: PathBuf, lasts: Vec<bool>) {
        self.rows.push(Row::Dir {
            path: path.clone(),
            lasts: lasts.clone(),
        });
        if !self.expanded.contains(&path) {
            return;
        }
        let Some(kids) = self.kids(&path) else {
            return;
        };
        let kids = filter::rank(kids, "", &*self.seams.is_repo);
        let n = kids.len();
        for (i, c) in kids.iter().take(CAP).enumerate() {
            let mut l = lasts.clone();
            l.push(i + 1 == n.min(CAP));
            self.push_dir(c.clone(), l);
        }
        if n > CAP {
            self.rows.push(Row::Cap {
                depth: lasts.len(),
                left: n - CAP,
            });
        }
    }

    fn first_selectable(&self) -> usize {
        self.rows.iter().position(Row::selectable).unwrap_or(0)
    }

    fn last_selectable(&self) -> usize {
        self.rows.iter().rposition(Row::selectable).unwrap_or(0)
    }

    /// Move the selection `delta` selectable rows.
    fn move_sel(&mut self, delta: isize) {
        let sel_rows: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.selectable())
            .map(|(i, _)| i)
            .collect();
        if sel_rows.is_empty() {
            self.sel = 0;
            return;
        }
        let pos = sel_rows
            .iter()
            .position(|i| *i >= self.sel)
            .unwrap_or(sel_rows.len() - 1);
        let next = (pos as isize + delta).clamp(0, sel_rows.len() as isize - 1);
        self.sel = sel_rows[next as usize];
    }

    /// Left: collapse the selected directory, or move to its parent.
    fn collapse_or_parent(&mut self) {
        let Some(row) = self.rows.get(self.sel).cloned() else {
            return;
        };
        let Row::Dir { path, lasts } = row else {
            return;
        };
        if self.expanded.contains(&path) {
            self.expanded.remove(&path);
            self.rebuild_rows();
            return;
        }
        let depth = lasts.len();
        for i in (0..self.sel).rev() {
            if let Row::Dir { lasts: pl, .. } = &self.rows[i]
                && pl.len() < depth
            {
                self.sel = i;
                return;
            }
        }
    }

    /// Right: expand the selected directory in place, move to its
    /// first child when already open, or enter it when it has no
    /// children shown.
    fn expand_or_enter(&mut self) {
        let Some(row) = self.rows.get(self.sel).cloned() else {
            return;
        };
        match row {
            Row::Used { path } => self.descend(&path),
            Row::Dir { path, lasts } => {
                if self.expanded.contains(&path) {
                    let next_is_child = self.rows.get(self.sel + 1).is_some_and(
                        |r| matches!(r, Row::Dir { lasts: pl, .. } if pl.len() > lasts.len()),
                    );
                    if next_is_child {
                        self.sel += 1;
                    } else {
                        self.descend(&path);
                    }
                    return;
                }
                match self.kids(&path) {
                    None => self.status = Some("(no access)".to_string()),
                    Some(kids) => {
                        if filter::rank(kids, "", &*self.seams.is_repo).is_empty() {
                            self.descend(&path);
                        } else {
                            self.expanded.insert(path);
                            self.rebuild_rows();
                        }
                    }
                }
            }
            Row::Cap { .. } => {}
        }
    }

    /// The input descends into `path` — the tree roots there and the
    /// filter starts empty.
    fn descend(&mut self, path: &Path) {
        let short = filter::shorten(path, &self.home);
        self.text = match short.ends_with('/') {
            true => short,
            false => format!("{short}/"),
        };
        self.cursor = self.text.len();
        self.edited();
    }

    /// Tab: complete the matches' longest common prefix; a single
    /// match descends into it.
    fn complete(&mut self) {
        if let [only] = self.matches.as_slice() {
            self.descend(&only.clone());
            return;
        }
        let names: Vec<String> = self
            .matches
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        let pre = filter::lcp(names.iter().map(String::as_str));
        if pre.len() <= self.filter.len() {
            return;
        }
        // Rewrite the filter component: the typed root prefix (text
        // minus the leftovers' spelling) + the prefix + the rest.
        let abs = filter::expand(&self.text, &self.home);
        let rest = filter::rest_after(&abs, &self.root);
        let leftover_text = abs
            .components()
            .skip(self.root.components().count())
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let prefix = self
            .text
            .strip_suffix(&leftover_text)
            .unwrap_or(&self.text)
            .to_string();
        let sep = if prefix.ends_with('/') { "" } else { "/" };
        let tail = if rest.is_empty() {
            String::new()
        } else {
            format!("/{rest}")
        };
        self.text = format!("{prefix}{sep}{pre}{tail}");
        self.cursor = self.text.len();
        self.edited();
    }

    /// Enter: the input answers a repository-with-commit pick; the
    /// highlighted row answers a pick next — the in-use group while
    /// the input is untouched, the filter's best match otherwise;
    /// an existing directory descends with the status; a tree row
    /// picks or expands.
    fn enter(&mut self) -> Option<Outcome> {
        match self.focus {
            Focus::Input => {
                let abs = filter::expand(&self.text, &self.home);
                if self.repo(&abs).pickable() {
                    return Some(Outcome::Picked(abs));
                }
                let sel_repo = self
                    .rows
                    .get(self.sel)
                    .and_then(Row::path)
                    .map(|p| p.to_path_buf());
                let sel_pickable = sel_repo.clone().is_some_and(|p| self.repo(&p).pickable());
                if sel_pickable && (!self.dirty || !(self.seams.is_dir)(&abs)) {
                    return Some(Outcome::Picked(sel_repo.expect("pickable above")));
                }
                if (self.seams.is_dir)(&abs) {
                    self.descend(&abs);
                    self.status = Some(NOT_PICKABLE.to_string());
                } else {
                    self.status = Some("no such directory".to_string());
                }
                None
            }
            Focus::Tree => {
                let path = match self.rows.get(self.sel) {
                    Some(Row::Used { path } | Row::Dir { path, .. }) => path.clone(),
                    _ => return None,
                };
                if self.repo(&path).pickable() {
                    return Some(Outcome::Picked(path));
                }
                // Expand what it has, then say why it cannot go.
                if !self.expanded.contains(&path)
                    && let Some(kids) = self.kids(&path)
                    && !filter::rank(kids, "", &*self.seams.is_repo).is_empty()
                {
                    self.expanded.insert(path);
                    self.rebuild_rows();
                }
                self.status = Some(NOT_PICKABLE.to_string());
                None
            }
        }
    }
}

//! The picker state machine over a fake filesystem: every key goes
//! through `key()`, rows come out — no terminal, no git, no real
//! directory ever read.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::state::{Focus, Key, Outcome, Picker};
use super::tree::{Repo, Row, Seams};

/// A fake filesystem: every directory's child directories, which
/// are repositories with a commit (branch + age), which are empty
/// repositories, which fail to list.
struct FakeFs {
    kids: BTreeMap<PathBuf, Vec<PathBuf>>,
    repos: BTreeMap<PathBuf, (String, u64)>,
    empty: BTreeSet<PathBuf>,
    no_access: BTreeSet<PathBuf>,
}

impl FakeFs {
    fn new() -> Self {
        FakeFs {
            kids: BTreeMap::new(),
            repos: BTreeMap::new(),
            empty: BTreeSet::new(),
            no_access: BTreeSet::new(),
        }
    }

    /// Register `dir` with the given child directory names.
    fn dir(&mut self, dir: &Path, names: &[&str]) {
        self.kids.insert(
            dir.to_path_buf(),
            names.iter().map(|n| dir.join(n)).collect(),
        );
    }

    fn repo(&mut self, dir: &Path, branch: &str, age_s: u64) {
        self.repos.insert(dir.to_path_buf(), (branch.into(), age_s));
    }

    fn empty_repo(&mut self, dir: &Path) {
        self.empty.insert(dir.to_path_buf());
    }

    fn no_access(&mut self, dir: &Path) {
        self.no_access.insert(dir.to_path_buf());
    }

    fn seams(&self) -> Seams<'_> {
        Seams {
            lister: Box::new(|p| {
                if self.no_access.contains(p) {
                    return None;
                }
                Some(self.kids.get(p).cloned().unwrap_or_default())
            }),
            repo: Box::new(|p| {
                if let Some((branch, age_s)) = self.repos.get(p) {
                    Repo::Commit {
                        branch: branch.clone(),
                        age_s: *age_s,
                    }
                } else if self.empty.contains(p) {
                    Repo::Empty
                } else {
                    Repo::None
                }
            }),
            is_dir: Box::new(|p| self.kids.contains_key(p)),
            is_repo: Box::new(|p| self.repos.contains_key(p) || self.empty.contains(p)),
        }
    }
}

fn home() -> PathBuf {
    PathBuf::from("/home/user")
}

/// The mock's tree under `/home/user`.
fn demo_fs() -> FakeFs {
    let home = home();
    let code = home.join("code");
    let clients = code.join("clients");
    let mut fs = FakeFs::new();
    fs.dir(&home, &["code", "Documents"]);
    fs.dir(
        &code,
        &[
            "acme-shop",
            "backlot",
            "scratch",
            "clients",
            "empty-repo",
            "noaccess",
            ".hidden",
            "node_modules",
            "target",
        ],
    );
    fs.dir(&clients, &["web-app", "web-app-v2"]);
    fs.dir(&code.join("scratch"), &["service-a"]);
    fs.dir(&code.join("acme-shop"), &["src"]);
    fs.dir(&code.join("backlot"), &["crates", "docs"]);
    fs.dir(&code.join("empty-repo"), &["src"]);
    fs.dir(&code.join("noaccess"), &["secret"]);
    fs.dir(&code.join(".hidden"), &[]);
    fs.dir(&code.join("node_modules"), &["dep"]);
    fs.dir(&code.join("target"), &["debug"]);
    fs.dir(&clients.join("web-app"), &[]);
    fs.dir(&clients.join("web-app-v2"), &[]);
    fs.dir(&home.join("Documents"), &[]);
    fs.repo(&code.join("acme-shop"), "main", 7200);
    fs.repo(&code.join("backlot"), "main", 300);
    fs.repo(&clients.join("web-app"), "develop", 86400);
    fs.empty_repo(&code.join("empty-repo"));
    fs.empty_repo(&clients.join("web-app-v2"));
    fs.no_access(&code.join("noaccess"));
    fs
}

fn picker<'a>(fs: &'a FakeFs, start: &Path, in_use: Vec<PathBuf>) -> Picker<'a> {
    Picker::new(fs.seams(), home(), start, in_use)
}

/// Type `s` into the picker.
fn typ(p: &mut Picker, s: &str) {
    for c in s.chars() {
        assert!(p.key(Key::Char(c)).is_none(), "typing {s:?}");
    }
}

fn row_paths(p: &Picker) -> Vec<String> {
    p.rows()
        .iter()
        .map(|r| match r {
            Row::Used { path } | Row::Dir { path, .. } => path.to_string_lossy().into_owned(),
            Row::Cap { left, .. } => format!("…{left}"),
        })
        .collect()
}

/// The picker starts at `~` when the current directory is no
/// repository — root home, the in-use group ahead of the tree.
#[test]
fn the_start_is_home_and_the_in_use_group_leads() {
    let fs = demo_fs();
    let in_use = vec![home().join("code/backlot"), home().join("code/acme-shop")];
    let p = picker(&fs, &home(), in_use);
    assert_eq!(p.input(), "~");
    assert_eq!(p.root(), &home());
    assert_eq!(p.in_use_rows(), 2);
    assert_eq!(p.focus(), Focus::Input);
    let paths = row_paths(&p);
    assert_eq!(
        paths,
        vec![
            "/home/user/code/backlot",
            "/home/user/code/acme-shop",
            "/home/user/code",
            "/home/user/Documents",
        ]
    );
    // Row kinds: two used rows, then the tree.
    assert!(matches!(p.rows()[0], Row::Used { .. }));
    assert!(matches!(p.rows()[2], Row::Dir { .. }));
}

/// Starting inside a repository puts its path in the input; the
/// tree roots at it and Enter picks it right away.
#[test]
fn a_repository_start_picks_the_typed_path() {
    let fs = demo_fs();
    let start = home().join("code/backlot");
    let mut p = picker(&fs, &start, vec![]);
    assert_eq!(p.input(), "~/code/backlot");
    assert_eq!(p.root(), &start);
    match p.key(Key::Enter) {
        Some(Outcome::Picked(path)) => assert_eq!(path, start),
        other => panic!("expected Picked, got {other:?}"),
    }
}

/// Typing filters live: the root moves to the deepest existing
/// directory and the leftover filters its children.
#[test]
fn typing_filters_live() {
    let fs = demo_fs();
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/code/");
    assert_eq!(p.input(), "~/code/");
    assert_eq!(p.root(), home().join("code"));
    assert_eq!(p.root_display(), "~/code");
    let paths = row_paths(&p);
    // Repositories first — `empty-repo` counts, it has a `.git` —
    // then by name; hidden and skipped are out.
    assert_eq!(
        paths,
        vec![
            "/home/user/code/acme-shop",
            "/home/user/code/backlot",
            "/home/user/code/empty-repo",
            "/home/user/code/clients",
            "/home/user/code/noaccess",
            "/home/user/code/scratch",
        ]
    );
    typ(&mut p, "ac");
    let paths = row_paths(&p);
    assert_eq!(
        paths,
        vec![
            "/home/user/code/acme-shop",
            "/home/user/code/backlot",
            "/home/user/code/noaccess",
            "/home/user/code/scratch"
        ]
    );
}

/// Tab completes the longest common prefix; one match descends.
#[test]
fn tab_completes_and_descends() {
    let fs = demo_fs();
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/od");
    // Only `code` matches "~/od" — Tab descends into it.
    p.key(Key::Tab);
    assert_eq!(p.input(), "~/code/");
    assert_eq!(p.root(), home().join("code"));
    // "~/code/ba" → a single match descends into it too.
    typ(&mut p, "ba");
    p.key(Key::Tab);
    assert_eq!(p.input(), "~/code/backlot/");
    assert_eq!(p.root(), home().join("code/backlot"));
    // An ambiguous completion stops at the longest common prefix:
    // `we` inside clients → web-app and web-app-v2 share `web-app`,
    // which exists — the tree roots there.
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/code/clients/we");
    p.key(Key::Tab);
    assert_eq!(p.input(), "~/code/clients/web-app");
    assert_eq!(p.root(), home().join("code/clients/web-app"));
    // A filter that leaves one match descends on Tab.
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/code/acm");
    p.key(Key::Tab);
    assert_eq!(p.input(), "~/code/acme-shop/");
    assert_eq!(p.root(), home().join("code/acme-shop"));
}

/// Backspace edits like any text field — cursor and filter follow.
#[test]
fn backspace_edits() {
    let fs = demo_fs();
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/code/bax");
    assert_eq!(p.input(), "~/code/bax");
    p.key(Key::Backspace);
    assert_eq!(p.input(), "~/code/ba");
    assert_eq!(p.cursor(), p.input().len());
    let paths = row_paths(&p);
    assert_eq!(paths, vec!["/home/user/code/backlot"]);
    // Backspace through the filter boundary re-roots the tree.
    for _ in 0.."code/ba".len() {
        p.key(Key::Backspace);
    }
    assert_eq!(p.input(), "~/");
    assert_eq!(p.root(), &home());
}

/// The in-use group is only there while the input is untouched.
#[test]
fn the_in_use_group_hides_once_typing_starts() {
    let fs = demo_fs();
    let in_use = vec![home().join("code/backlot")];
    let mut p = picker(&fs, &home(), in_use);
    assert!(matches!(p.rows()[0], Row::Used { .. }));
    typ(&mut p, "x");
    assert!(p.rows().iter().all(|r| !matches!(r, Row::Used { .. })));
    assert_eq!(p.in_use_rows(), 0);
}

/// Enter picks a used-repository row like any repository row.
#[test]
fn a_used_repository_picks_like_any_row() {
    let fs = demo_fs();
    let in_use = vec![home().join("code/backlot")];
    let mut p = picker(&fs, &home(), in_use);
    // The first selectable row is the used repo — Enter picks it.
    match p.key(Key::Enter) {
        Some(Outcome::Picked(path)) => {
            assert_eq!(path, home().join("code/backlot"))
        }
        other => panic!("expected Picked, got {other:?}"),
    }
}

/// Up/Down move the selection; Right expands a directory in place,
/// Left collapses it or moves to the parent.
#[test]
fn the_tree_moves_expands_and_collapses() {
    let fs = demo_fs();
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/code/");
    // Select backlot's row: repos lead, so sel is acme-shop.
    p.key(Key::Down);
    assert_eq!(p.focus(), Focus::Tree);
    assert_eq!(row_paths(&p)[p.sel()], "/home/user/code/backlot");
    // clients — expand it in place.
    let clients = row_paths(&p)
        .iter()
        .position(|s| s == "/home/user/code/clients")
        .unwrap();
    while p.sel() < clients {
        p.key(Key::Down);
    }
    p.key(Key::Right);
    let paths = row_paths(&p);
    assert_eq!(
        paths
            .iter()
            .filter(|s| s.starts_with("/home/user/code/clients"))
            .count(),
        3 // clients itself + web-app + web-app-v2
    );
    // Left collapses it back.
    let clients_idx = paths
        .iter()
        .position(|s| s == "/home/user/code/clients")
        .unwrap();
    while p.sel() > clients_idx {
        p.key(Key::Up);
    }
    p.key(Key::Left);
    let paths = row_paths(&p);
    assert_eq!(
        paths
            .iter()
            .filter(|s| s.starts_with("/home/user/code/clients/"))
            .count(),
        0
    );
    // Left again on the collapsed row moves to the parent —
    // clients's parent is root-level, so nothing above: stays.
    p.key(Key::Left);
}

/// Right on a leaf directory enters it — the input descends.
#[test]
fn right_enters_a_leaf_directory() {
    let fs = demo_fs();
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/code/");
    // scratch has one child service-a — Right expands, Right again
    // on the leaf service-a enters it.
    let sc = row_paths(&p)
        .iter()
        .position(|s| s == "/home/user/code/scratch")
        .unwrap();
    while p.sel() < sc {
        p.key(Key::Down);
    }
    p.key(Key::Right); // expands → service-a appears
    p.key(Key::Down); // select service-a
    p.key(Key::Right); // leaf → descend
    assert_eq!(p.input(), "~/code/scratch/service-a/");
    assert_eq!(p.focus(), Focus::Input);
}

/// Enter on a repository row picks it; on a plain directory it
/// expands and the status says it is not a repository with a commit.
#[test]
fn enter_picks_a_repository_and_warns_on_a_plain_dir() {
    let fs = demo_fs();
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/code/ba");
    // backlot selected first — Enter picks it.
    match p.key(Key::Enter) {
        Some(Outcome::Picked(path)) => {
            assert_eq!(path, home().join("code/backlot"))
        }
        other => panic!("expected Picked, got {other:?}"),
    }
    // A plain directory: Enter expands it and explains.
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/code/");
    let sc = row_paths(&p)
        .iter()
        .position(|s| s == "/home/user/code/scratch")
        .unwrap();
    while p.sel() < sc {
        p.key(Key::Down);
    }
    p.key(Key::Enter);
    assert_eq!(p.status(), Some("not a git repository with a commit"));
    let paths = row_paths(&p);
    assert!(paths.contains(&"/home/user/code/scratch/service-a".to_string()));
}

/// A repository without commits cannot be picked; an unreadable
/// directory shows its status and never aborts.
#[test]
fn an_empty_repository_and_no_access_directory_report() {
    let fs = demo_fs();
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/code/");
    let er = row_paths(&p)
        .iter()
        .position(|s| s == "/home/user/code/empty-repo")
        .unwrap();
    while p.sel() < er {
        p.key(Key::Down);
    }
    assert!(p.key(Key::Enter).is_none(), "no commits cannot pick");
    assert_eq!(p.status(), Some("not a git repository with a commit"));
    // noaccess: Enter expands nothing, the row marker says no access.
    let na = row_paths(&p)
        .iter()
        .position(|s| s == "/home/user/code/noaccess")
        .unwrap();
    while p.sel() < na {
        p.key(Key::Down);
    }
    assert!(p.key(Key::Enter).is_none());
    assert_eq!(p.status(), Some("not a git repository with a commit"));
    assert!(row_paths(&p).iter().all(|s| !s.contains("secret")));
}

/// An unreadable root shows no rows and flags `(no access)` for the
/// tree area — the picker never aborts.
#[test]
fn an_unreadable_root_lists_nothing() {
    let fs = demo_fs();
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/code/noaccess/");
    assert!(!p.root_listed());
    assert!(row_paths(&p).iter().all(|s| !s.contains("secret")));
}

/// Esc returns to the input; the second Esc cancels and nothing was
/// picked — Ctrl+C cancels at once.
#[test]
fn esc_twice_cancels_and_ctrl_c_at_once() {
    let fs = demo_fs();
    let mut p = picker(&fs, &home(), vec![]);
    p.key(Key::Down); // tree focus
    assert!(p.key(Key::Esc).is_none());
    assert_eq!(p.focus(), Focus::Input);
    match p.key(Key::Esc) {
        Some(Outcome::Cancelled) => {}
        other => panic!("expected Cancelled, got {other:?}"),
    }
    let mut p = picker(&fs, &home(), vec![]);
    p.key(Key::Down);
    match p.key(Key::CtrlC) {
        Some(Outcome::Cancelled) => {}
        other => panic!("expected Cancelled, got {other:?}"),
    }
}

/// PgUp/PgDn and Home/End jump through the selection.
#[test]
fn pagination_and_ends() {
    let fs = demo_fs();
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/code/");
    p.set_page(3);
    p.key(Key::End);
    let last = p.sel();
    p.key(Key::Home);
    assert_eq!(p.sel(), 0);
    p.key(Key::PgDn);
    assert!(p.sel() >= 3, "PgDn jumped {}", p.sel());
    p.key(Key::End);
    assert_eq!(p.sel(), last);
    p.key(Key::PgUp);
    assert!(p.sel() < last, "PgUp went back");
}

/// The in-use group caps at eight repositories.
#[test]
fn the_in_use_group_caps_at_eight() {
    let fs = demo_fs();
    let in_use: Vec<PathBuf> = (0..10).map(|i| home().join(format!("code/r{i}"))).collect();
    let p = picker(&fs, &home(), in_use);
    assert_eq!(
        p.rows()
            .iter()
            .filter(|r| matches!(r, Row::Used { .. }))
            .count(),
        8
    );
}

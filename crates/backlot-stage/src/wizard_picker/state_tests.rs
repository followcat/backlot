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
    let aic = home.join("AIcontrol");
    let phone = aic.join("phone");
    let mut fs = FakeFs::new();
    fs.dir(&home, &["AIcontrol", "Documents"]);
    fs.dir(
        &aic,
        &[
            "acme-shop",
            "backlot",
            "backlot-work",
            "phone",
            "empty-repo",
            "noaccess",
            ".hidden",
            "node_modules",
            "target",
        ],
    );
    fs.dir(&phone, &["wherdr", "wherdr-v3"]);
    fs.dir(&aic.join("backlot-work"), &["u1-api"]);
    fs.dir(&aic.join("acme-shop"), &["src"]);
    fs.dir(&aic.join("backlot"), &["crates", "docs"]);
    fs.dir(&aic.join("empty-repo"), &["src"]);
    fs.dir(&aic.join("noaccess"), &["secret"]);
    fs.dir(&aic.join(".hidden"), &[]);
    fs.dir(&aic.join("node_modules"), &["dep"]);
    fs.dir(&aic.join("target"), &["debug"]);
    fs.dir(&home.join("Documents"), &[]);
    fs.repo(&aic.join("acme-shop"), "main", 7200);
    fs.repo(&aic.join("backlot"), "main", 300);
    fs.repo(&phone.join("wherdr"), "mobile", 86400);
    fs.empty_repo(&aic.join("empty-repo"));
    fs.no_access(&aic.join("noaccess"));
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
    let in_use = vec![
        home().join("AIcontrol/backlot"),
        home().join("AIcontrol/acme-shop"),
    ];
    let p = picker(&fs, &home(), in_use);
    assert_eq!(p.input(), "~");
    assert_eq!(p.root(), &home());
    assert_eq!(p.in_use_rows(), 2);
    assert_eq!(p.focus(), Focus::Input);
    let paths = row_paths(&p);
    assert_eq!(
        paths,
        vec![
            "/home/user/AIcontrol/backlot",
            "/home/user/AIcontrol/acme-shop",
            "/home/user/AIcontrol",
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
    let start = home().join("AIcontrol/backlot");
    let mut p = picker(&fs, &start, vec![]);
    assert_eq!(p.input(), "~/AIcontrol/backlot");
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
    typ(&mut p, "/AIcontrol/");
    assert_eq!(p.input(), "~/AIcontrol/");
    assert_eq!(p.root(), home().join("AIcontrol"));
    assert_eq!(p.root_display(), "~/AIcontrol");
    let paths = row_paths(&p);
    // Repositories first — `empty-repo` counts, it has a `.git` —
    // then by name; hidden and skipped are out.
    assert_eq!(
        paths,
        vec![
            "/home/user/AIcontrol/acme-shop",
            "/home/user/AIcontrol/backlot",
            "/home/user/AIcontrol/empty-repo",
            "/home/user/AIcontrol/backlot-work",
            "/home/user/AIcontrol/noaccess",
            "/home/user/AIcontrol/phone",
        ]
    );
    typ(&mut p, "bac");
    let paths = row_paths(&p);
    assert_eq!(
        paths,
        vec![
            "/home/user/AIcontrol/backlot",
            "/home/user/AIcontrol/backlot-work"
        ]
    );
}

/// Tab completes the longest common prefix; one match descends.
#[test]
fn tab_completes_and_descends() {
    let fs = demo_fs();
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/A");
    // Only AIcontrol matches "~/A" — Tab descends into it.
    p.key(Key::Tab);
    assert_eq!(p.input(), "~/AIcontrol/");
    assert_eq!(p.root(), home().join("AIcontrol"));
    // "~/AIcontrol/ba" → backlot and backlot-work share "backlot".
    typ(&mut p, "ba");
    p.key(Key::Tab);
    assert_eq!(p.input(), "~/AIcontrol/backlot");
    // "~/AIcontrol/backlot" exists now — the tree is inside it.
    assert_eq!(p.root(), home().join("AIcontrol/backlot"));
    // A filter that leaves one match descends on Tab.
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/AIcontrol/acm");
    p.key(Key::Tab);
    assert_eq!(p.input(), "~/AIcontrol/acme-shop/");
    assert_eq!(p.root(), home().join("AIcontrol/acme-shop"));
}

/// Backspace edits like any text field — cursor and filter follow.
#[test]
fn backspace_edits() {
    let fs = demo_fs();
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/AIcontrol/bax");
    assert_eq!(p.input(), "~/AIcontrol/bax");
    p.key(Key::Backspace);
    assert_eq!(p.input(), "~/AIcontrol/ba");
    assert_eq!(p.cursor(), p.input().len());
    let paths = row_paths(&p);
    assert_eq!(
        paths,
        vec!["/home/user/AIcontrol/backlot", "/home/user/AIcontrol/backlot-work"]
    );
    // Backspace through the filter boundary re-roots the tree.
    for _ in 0.."AIcontrol/ba".len() {
        p.key(Key::Backspace);
    }
    assert_eq!(p.input(), "~/");
    assert_eq!(p.root(), &home());
}

/// The in-use group is only there while the input is untouched.
#[test]
fn the_in_use_group_hides_once_typing_starts() {
    let fs = demo_fs();
    let in_use = vec![home().join("AIcontrol/backlot")];
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
    let in_use = vec![home().join("AIcontrol/backlot")];
    let mut p = picker(&fs, &home(), in_use);
    // The first selectable row is the used repo — Enter picks it.
    match p.key(Key::Enter) {
        Some(Outcome::Picked(path)) => {
            assert_eq!(path, home().join("AIcontrol/backlot"))
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
    typ(&mut p, "/AIcontrol/");
    // Select backlot's row: repos lead, so sel is acme-shop.
    p.key(Key::Down);
    assert_eq!(p.focus(), Focus::Tree);
    assert_eq!(row_paths(&p)[p.sel()], "/home/user/AIcontrol/backlot");
    // phone is the last row — expand it in place.
    let phone = row_paths(&p)
        .iter()
        .position(|s| s == "/home/user/AIcontrol/phone")
        .unwrap();
    while p.sel() < phone {
        p.key(Key::Down);
    }
    p.key(Key::Right);
    let paths = row_paths(&p);
    assert_eq!(
        paths
            .iter()
            .filter(|s| s.starts_with("/home/user/AIcontrol/phone"))
            .count(),
        3 // phone itself + wherdr + wherdr-v3
    );
    // Left collapses it back.
    let phone_idx = paths
        .iter()
        .position(|s| s == "/home/user/AIcontrol/phone")
        .unwrap();
    while p.sel() > phone_idx {
        p.key(Key::Up);
    }
    p.key(Key::Left);
    let paths = row_paths(&p);
    assert_eq!(
        paths
            .iter()
            .filter(|s| s.starts_with("/home/user/AIcontrol/phone/"))
            .count(),
        0
    );
    // Left again on the collapsed row moves to the parent — phone's
    // parent is root-level, so nothing above: stays.
    p.key(Key::Left);
}

/// Right on a leaf directory enters it — the input descends.
#[test]
fn right_enters_a_leaf_directory() {
    let fs = demo_fs();
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/AIcontrol/");
    // backlot-work has one child u1-api — Right expands, Right again
    // on the leaf u1-api enters it.
    let bw = row_paths(&p)
        .iter()
        .position(|s| s == "/home/user/AIcontrol/backlot-work")
        .unwrap();
    while p.sel() < bw {
        p.key(Key::Down);
    }
    p.key(Key::Right); // expands → u1-api appears
    p.key(Key::Down); // select u1-api
    p.key(Key::Right); // leaf → descend
    assert_eq!(p.input(), "~/AIcontrol/backlot-work/u1-api/");
    assert_eq!(p.focus(), Focus::Input);
}

/// Enter on a repository row picks it; on a plain directory it
/// expands and the status says it is not a repository with a commit.
#[test]
fn enter_picks_a_repository_and_warns_on_a_plain_dir() {
    let fs = demo_fs();
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/AIcontrol/ba");
    // backlot selected first — Enter picks it.
    match p.key(Key::Enter) {
        Some(Outcome::Picked(path)) => {
            assert_eq!(path, home().join("AIcontrol/backlot"))
        }
        other => panic!("expected Picked, got {other:?}"),
    }
    // A plain directory: Enter expands it and explains.
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/AIcontrol/");
    let bw = row_paths(&p)
        .iter()
        .position(|s| s == "/home/user/AIcontrol/backlot-work")
        .unwrap();
    while p.sel() < bw {
        p.key(Key::Down);
    }
    p.key(Key::Enter);
    assert_eq!(p.status(), Some("not a git repository with a commit"));
    let paths = row_paths(&p);
    assert!(paths.contains(&"/home/user/AIcontrol/backlot-work/u1-api".to_string()));
}

/// A repository without commits cannot be picked; an unreadable
/// directory shows its status and never aborts.
#[test]
fn an_empty_repository_and_no_access_directory_report() {
    let fs = demo_fs();
    let mut p = picker(&fs, &home(), vec![]);
    typ(&mut p, "/AIcontrol/");
    let er = row_paths(&p)
        .iter()
        .position(|s| s == "/home/user/AIcontrol/empty-repo")
        .unwrap();
    while p.sel() < er {
        p.key(Key::Down);
    }
    assert!(p.key(Key::Enter).is_none(), "no commits cannot pick");
    assert_eq!(p.status(), Some("not a git repository with a commit"));
    // noaccess: Enter expands nothing, the row marker says no access.
    let na = row_paths(&p)
        .iter()
        .position(|s| s == "/home/user/AIcontrol/noaccess")
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
    typ(&mut p, "/AIcontrol/noaccess/");
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
    typ(&mut p, "/AIcontrol/");
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
    let in_use: Vec<PathBuf> = (0..10)
        .map(|i| home().join(format!("AIcontrol/r{i}")))
        .collect();
    let p = picker(&fs, &home(), in_use);
    assert_eq!(
        p.rows()
            .iter()
            .filter(|r| matches!(r, Row::Used { .. }))
            .count(),
        8
    );
}

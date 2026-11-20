//! The picker's rendered face as text: the 100×30 screen the brief
//! draws, the 60×20 floor it must stay readable at, and the tiny
//! sizes it must never panic on. Everything is a `Buffer` — no
//! terminal.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::render::{draw, view_height};
use super::state::{Key, Picker};
use super::tree::{Repo, Row, Seams};

/// The fake filesystem — the same shape `state_tests` drives.
struct FakeFs {
    kids: BTreeMap<PathBuf, Vec<PathBuf>>,
    repos: BTreeMap<PathBuf, (String, u64)>,
    empty: BTreeSet<PathBuf>,
}

impl FakeFs {
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
    fn seams(&self) -> Seams<'_> {
        Seams {
            lister: Box::new(|p| Some(self.kids.get(p).cloned().unwrap_or_default())),
            repo: Box::new(|p| {
                if let Some((branch, age_s)) = self.repos.get(p) {
                    return Repo::Commit {
                        branch: branch.clone(),
                        age_s: *age_s,
                    };
                }
                if self.empty.contains(p) {
                    return Repo::Empty;
                }
                let Some(kids) = self.kids.get(p) else {
                    return Repo::Missing;
                };
                let repos = kids
                    .iter()
                    .filter(|k| self.repos.contains_key(*k) || self.empty.contains(*k))
                    .count();
                match repos {
                    0 => Repo::Dir,
                    n => Repo::Workspace { repos: n },
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

/// `~/code`: two repositories, a scratch dir and `clients` with its
/// own two — the brief's sketch.
fn demo_fs() -> (FakeFs, PathBuf) {
    let home = home();
    let code = home.join("code");
    let clients = code.join("clients");
    let mut fs = FakeFs {
        kids: BTreeMap::new(),
        repos: BTreeMap::new(),
        empty: BTreeSet::new(),
    };
    fs.dir(&home, &["code", "Documents"]);
    fs.dir(&code, &["acme-shop", "backlot", "clients", "scratch"]);
    fs.dir(&clients, &["web-app", "web-app-v2"]);
    fs.dir(&home.join("Documents"), &[]);
    fs.dir(&code.join("acme-shop"), &[]);
    fs.dir(&code.join("backlot"), &[]);
    fs.dir(&code.join("scratch"), &["service-a"]);
    fs.dir(&code.join("scratch/service-a"), &[]);
    fs.dir(&clients.join("web-app"), &[]);
    fs.dir(&clients.join("web-app-v2"), &[]);
    fs.repo(&code.join("acme-shop"), "main", 7200);
    fs.repo(&code.join("backlot"), "main", 300);
    fs.repo(&clients.join("web-app"), "develop", 86400);
    fs.empty_repo(&clients.join("web-app-v2"));
    (fs, code)
}

/// A plain-text dump of the buffer's cells, row per line — what a
/// screen-capture shows.
fn dump(buffer: &Buffer, area: Rect) -> String {
    let mut out = String::new();
    for y in area.y..area.bottom() {
        let mut line = String::new();
        for x in area.x..area.right() {
            line.push_str(buffer[(x, y)].symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn screen(p: &mut Picker, w: u16, h: u16) -> String {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    p.set_page(view_height(area));
    draw(p, area, &mut buf);
    dump(&buf, area)
}

fn typ(p: &mut Picker, s: &str) {
    for c in s.chars() {
        p.key(Key::Char(c));
    }
}

/// The 100×30 screen: the untouched input shows the in-use group,
/// `code` and `clients` expanded in place, the meta column on.
#[test]
fn the_100x30_screen() {
    let (fs, code) = demo_fs();
    let in_use = vec![code.join("backlot"), code.join("acme-shop")];
    let mut p = Picker::new(fs.seams(), home(), &home(), in_use);
    // Select code (below the two used rows), expand it; then
    // select clients inside and expand it.
    while p.rows().get(p.sel()).and_then(Row::path) != Some(code.as_path()) {
        p.key(Key::Down);
    }
    p.key(Key::Right);
    while p.rows().get(p.sel()).and_then(Row::path) != Some(code.join("clients").as_path()) {
        p.key(Key::Down);
    }
    p.key(Key::Right);
    let out = screen(&mut p, 100, 30);
    assert_eq!(
        out,
        " New studio · repository
 path › ~/▏                                                                            Tab completes

   ~
     ● ~/code/backlot                                     ● main · 5 min ago
     ● ~/code/acme-shop                                   ● main · 2 h ago
   ├ ▾ code                                               ○ workspace · 2 repos
   │   ├   acme-shop                                      ● main · 2 h ago
   │   ├   backlot                                        ● main · 5 min ago
 ▶ │   ├ ▾ clients                                        ○ workspace · 2 repos
   │   │   ├   web-app                                    ● develop · 1 d ago
   │   │   └   web-app-v2                                 ○ 0 commits · will create initial commit
   │   └ ▸ scratch                                        ○ directory · will git init
   └   Documents                                          ○ directory · will git init














 ● every directory binds a path — Enter picks it
 ↑↓ move   → open   ← close/up   Tab complete   Enter pick   Esc back to typing
"
    );
}

/// The 60×20 floor: meta still fits, the hints row clips.
#[test]
fn the_60x20_screen() {
    let (fs, _code) = demo_fs();
    let mut p = Picker::new(fs.seams(), home(), &home(), vec![]);
    typ(&mut p, "code/");
    let out = screen(&mut p, 60, 20);
    assert_eq!(
        out,
        " New studio · repository
 path › ~/code/▏                               Tab completes

   ~/code
 ▶ ├   acme-shop  ● main · 2 h ago
   ├   backlot    ● main · 5 min ago
   ├ ▸ clients    ○ workspace · 2 repos
   └ ▸ scratch    ○ directory · will git init










 ● every directory binds a path — Enter picks it
 ↑↓ move   → open   ← close/up   Tab complete   Enter pick
"
    );
    assert_eq!(out.lines().count(), 20, "{out}");
}

/// Narrower still: the meta column drops first, names truncate.
#[test]
fn a_40_wide_screen_drops_the_meta() {
    let (fs, _code) = demo_fs();
    let mut p = Picker::new(fs.seams(), home(), &home(), vec![]);
    typ(&mut p, "code/");
    let out = screen(&mut p, 40, 12);
    assert!(!out.contains("main"), "meta dropped: {out}");
    assert!(out.contains("▶ ├   acme-shop"), "{out}");
    // A long name truncates with …
    typ(&mut p, "clients/web-app-");
    let out = screen(&mut p, 30, 12);
    assert!(out.contains("web-app-v2"), "{out}");
}

/// Tiny areas never panic — a 10×4 draws what fits.
#[test]
fn a_tiny_screen_never_panics() {
    let (fs, _code) = demo_fs();
    let mut p = Picker::new(fs.seams(), home(), &home(), vec![]);
    for (w, h) in [(10, 4), (1, 1), (0, 0), (5, 3)] {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        p.set_page(view_height(area));
        draw(&mut p, area, &mut buf);
    }
}

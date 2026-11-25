//! `backlot repos [--json] [--from <dir>] [--all]` (G4): the directory
//! listing the New Studio dialog browses on whichever machine the
//! command runs on. It never leaves the user's own tree: `--from`
//! resolves through `~` and `..` to a canonical directory that must
//! stay inside `$HOME` — another user's path and a `..` escape above
//! it are refused alike. Repositories are classified like the
//! wizard's picker: a `.git` mark and `git log -1` tell `git_ready`
//! from `git_empty`; a plain directory is `dir`. A studio's recorded
//! repo marks `in_use_studio`.
//!
//! Dot-directories (`.cache`, `.config`, …) stay hidden unless `--all`
//! asks for them, so a `$HOME` listing opens on real work rather than
//! on dotfiles. The dialog passes `--all` and applies the same rule to
//! its live filter, so typing `.` still reveals them there.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::store;
use crate::workareas;

/// One listed child directory.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Entry {
    name: String,
    path: String,
    kind: String,
    #[serde(default)]
    in_use_studio: Option<String>,
    /// The checked-out branch — `null` on a plain directory or a
    /// detached/unknown HEAD.
    #[serde(default)]
    branch: Option<String>,
    /// A `dir` entry's direct-child repositories, by name — the
    /// dialog reads it as the workspace's member list (D4); absent
    /// on repository entries and on plain directories.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    members: Vec<String>,
}

/// The `--json` document — the typed result a remote `repos` ack
/// carries.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Listing {
    pub cwd: String,
    pub entries: Vec<Entry>,
}

/// The `~` prefix's expansion — `$HOME`, or `None` when unset.
fn home(env: &Env) -> Option<PathBuf> {
    env.get("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// `--from` → the canonical directory it names. `~` expands; the
/// argument must be absolute after expansion and must canonicalize
/// inside `$HOME` — `..` above it or a sibling user's tree is
/// refused, so the listing can never follow into another user's
/// paths. A missing HOME imposes no scope (the fallback anchor `/`
/// admits everything).
fn resolve_from(env: &Env, from: Option<&str>) -> Result<(PathBuf, PathBuf), Fail> {
    let anchor = home(env)
        .and_then(|h| h.canonicalize().ok())
        .unwrap_or_else(|| PathBuf::from("/"));
    let raw = match from {
        None => home(env).unwrap_or_else(|| PathBuf::from("/")),
        Some(arg) => match arg.strip_prefix("~") {
            Some("") => home(env).ok_or_else(|| Fail::usage("--from '~' but HOME is unset"))?,
            Some(rest) if rest.starts_with('/') => {
                let home = home(env).ok_or_else(|| Fail::usage("--from '~' but HOME is unset"))?;
                home.join(rest.trim_start_matches('/'))
            }
            Some(_) => return Err(Fail::usage(format!("bad --from '{arg}'"))),
            None => PathBuf::from(arg),
        },
    };
    if !raw.is_absolute() {
        return Err(Fail::usage(format!(
            "--from must be an absolute path, got '{raw:?}'"
        )));
    }
    let dir = raw
        .canonicalize()
        .map_err(|e| Fail::refused(format!("--from '{}': {e}", raw.display())))?;
    if !dir.is_dir() {
        return Err(Fail::refused(format!(
            "--from '{}' is not a directory",
            dir.display()
        )));
    }
    if dir != anchor && !dir.starts_with(&anchor) {
        return Err(Fail::refused(format!(
            "--from '{}' is outside $HOME — repos only lists your own tree",
            dir.display()
        )));
    }
    Ok((dir, anchor))
}

/// `path`'s git mark: a `.git` git itself accepts (a linked worktree
/// counts; a broken mark does not — the path is a plain `dir`, the
/// same answer `paths::classify` gives the launch), then one `log -1`
/// for the commit age — a repository with no commits yet is
/// `git_empty`. The branch is `symbolic-ref`'s answer, `None` on a
/// detached HEAD.
fn probe(path: &Path) -> (&'static str, Option<String>) {
    if !crate::paths::is_git(path) {
        return ("dir", None);
    }
    let dir = path.to_string_lossy();
    let log = workareas::real_git(&["-C", &dir, "log", "-1", "--format=%ct"], ".");
    if log.code != 0 {
        return ("git_empty", branch(&dir));
    }
    ("git_ready", branch(&dir))
}

fn branch(dir: &str) -> Option<String> {
    let out = workareas::real_git(
        &["-C", dir, "symbolic-ref", "--quiet", "--short", "HEAD"],
        ".",
    );
    if out.code != 0 {
        return None;
    }
    let branch = out.stdout.trim();
    (!branch.is_empty()).then(|| branch.to_string())
}

/// Canonical repo path → the studio using it, best-effort: a state
/// base that cannot resolve (no override, no reachable core) or a
/// studio whose log does not read simply contributes nothing.
fn in_use(base: Option<&Path>) -> std::collections::BTreeMap<PathBuf, String> {
    let mut map = std::collections::BTreeMap::new();
    let Some(base) = base else {
        return map;
    };
    let Ok(names) = store::studios(base) else {
        return map;
    };
    for name in names {
        let dir = store::studio_dir(base, &name);
        let Ok(state) = store::load_state(&dir) else {
            continue;
        };
        let Some(config) = &state.studio else {
            continue;
        };
        for repo in std::iter::once(&config.repo).chain(config.members.iter()) {
            if let Ok(canonical) = PathBuf::from(repo).canonicalize() {
                map.insert(canonical, name.clone());
            }
        }
    }
    map
}

/// The listing: `cwd` is the canonical `--from`, `entries` its child
/// directories — git repositories first (`git_ready`, then
/// `git_empty`), workspaces over member repositories next, plain
/// directories last, each group by name. Dot-directories are skipped
/// unless `all` asks for them.
pub(crate) fn scan(
    env: &Env,
    base: Option<&Path>,
    from: Option<&str>,
    all: bool,
) -> Result<Listing, Fail> {
    let (dir, _anchor) = resolve_from(env, from)?;
    let in_use = in_use(base);
    let rd = std::fs::read_dir(&dir)
        .map_err(|e| Fail::refused(format!("--from '{}': {e}", dir.display())))?;
    let mut entries = Vec::new();
    for e in rd.flatten() {
        let path = e.path();
        if !path.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if !all && name.starts_with('.') {
            continue;
        }
        let canonical = path.canonicalize().unwrap_or(path.clone());
        let (kind, branch) = probe(&path);
        let members = match kind {
            "dir" => crate::paths::member_repos(&path)
                .iter()
                .filter_map(|m| {
                    Path::new(m)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                })
                .collect(),
            _ => Vec::new(),
        };
        let rank = match kind {
            "git_ready" => 0,
            "git_empty" => 1,
            // A directory holding member repositories is a workspace —
            // the shape a studio usually wants, above a plain pick.
            _ if !members.is_empty() => 2,
            _ => 3,
        };
        entries.push((
            rank,
            Entry {
                name,
                path: canonical.to_string_lossy().into_owned(),
                kind: kind.to_string(),
                in_use_studio: in_use.get(&canonical).cloned(),
                branch,
                members,
            },
        ));
    }
    entries.sort_by(|(ka, a), (kb, b)| ka.cmp(kb).then(a.name.cmp(&b.name)));
    Ok(Listing {
        cwd: dir.to_string_lossy().into_owned(),
        entries: entries.into_iter().map(|(_, e)| e).collect(),
    })
}

/// `backlot repos`: JSON for the dialog, a readable table without
/// `--json` — the same rows either way.
pub fn run(
    env: &Env,
    common: &Common,
    json: bool,
    from: Option<&str>,
    all: bool,
) -> Result<String, Fail> {
    let base = crate::cli::base(env, common).ok();
    let listing = scan(env, base.as_deref(), from, all)?;
    format_listing(&listing, json)
}

/// JSON for the dialog, a readable table without `--json` — the same
/// rows either way. Shared by the CLI and the remote ack's printer.
pub(crate) fn format_listing(listing: &Listing, json: bool) -> Result<String, Fail> {
    if json {
        return serde_json::to_string_pretty(listing)
            .map_err(|e| Fail::refused(format!("repos: {e}")));
    }
    let mut out = listing.cwd.clone();
    for e in &listing.entries {
        let mut line = format!("\n{:<9} {}", e.kind, e.name);
        if let Some(studio) = &e.in_use_studio {
            line.push_str(&format!("  (in use by {studio})"));
        }
        out.push_str(&line);
    }
    Ok(out)
}

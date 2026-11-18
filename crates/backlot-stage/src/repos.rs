//! `backlot repos [--json] [--from <dir>]` (G4): the directory
//! listing the New Studio dialog browses on whichever machine the
//! command runs on. It never leaves the user's own tree: `--from`
//! resolves through `~` and `..` to a canonical directory that must
//! stay inside `$HOME` — another user's path and a `..` escape above
//! it are refused alike. Repositories are classified like the
//! wizard's picker: a `.git` mark and `git log -1` tell `git_ready`
//! from `git_empty`; a plain directory is `dir`. A studio's recorded
//! repo marks `in_use_studio`.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::store;
use crate::workareas;

/// One listed child directory.
#[derive(Debug, Serialize)]
struct Entry {
    name: String,
    path: String,
    kind: &'static str,
    in_use_studio: Option<String>,
    /// The checked-out branch — `null` on a plain directory or a
    /// detached/unknown HEAD.
    branch: Option<String>,
}

/// The `--json` document.
#[derive(Debug, Serialize)]
struct Listing {
    cwd: String,
    entries: Vec<Entry>,
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

/// `path`'s git mark: `.git` present (a linked worktree counts), then
/// one `log -1` for the commit age — a repository with no commits yet
/// is `git_empty`. The branch is `symbolic-ref`'s answer, `None` on a
/// detached HEAD.
fn probe(path: &Path) -> (&'static str, Option<String>) {
    if !path.join(".git").exists() {
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
fn in_use(env: &Env, common: &Common) -> std::collections::BTreeMap<PathBuf, String> {
    let mut map = std::collections::BTreeMap::new();
    let Ok(base) = crate::cli::base(env, common) else {
        return map;
    };
    let Ok(names) = store::studios(&base) else {
        return map;
    };
    for name in names {
        let dir = store::studio_dir(&base, &name);
        let Ok(state) = store::load_state(&dir) else {
            continue;
        };
        let Some(config) = &state.studio else {
            continue;
        };
        let repo = PathBuf::from(&config.repo);
        if let Ok(canonical) = repo.canonicalize() {
            map.insert(canonical, name);
        }
    }
    map
}

/// The listing: `cwd` is the canonical `--from`, `entries` its child
/// directories — git repositories first (`git_ready`, then
/// `git_empty`), plain directories last, each group by name.
fn list(env: &Env, common: &Common, from: Option<&str>) -> Result<Listing, Fail> {
    let (dir, _anchor) = resolve_from(env, from)?;
    let in_use = in_use(env, common);
    let rd = std::fs::read_dir(&dir)
        .map_err(|e| Fail::refused(format!("--from '{}': {e}", dir.display())))?;
    let mut entries = Vec::new();
    for e in rd.flatten() {
        let path = e.path();
        if !path.is_dir() {
            continue;
        }
        let canonical = path.canonicalize().unwrap_or(path.clone());
        let (kind, branch) = probe(&path);
        let name = e.file_name().to_string_lossy().into_owned();
        entries.push((
            match kind {
                "git_ready" => 0,
                "git_empty" => 1,
                _ => 2,
            },
            Entry {
                name,
                path: canonical.to_string_lossy().into_owned(),
                kind,
                in_use_studio: in_use.get(&canonical).cloned(),
                branch,
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
pub fn run(env: &Env, common: &Common, json: bool, from: Option<&str>) -> Result<String, Fail> {
    let listing = list(env, common, from)?;
    if json {
        return serde_json::to_string_pretty(&listing)
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

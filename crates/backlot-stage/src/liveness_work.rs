//! Work-area git probes and delta helpers for [`crate::liveness`]: inspects
//! `HEAD`, resolved branch refs, `.git/index` mtime, and 60 s throttled
//! `git --no-optional-locks status --porcelain` without walking `target/`,
//! `node_modules/`, or `.git/objects/`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use backlot_model::state::{StudioState, actor_task};

use crate::workareas::{GitRun, role_dir};

/// One sample of a role's local work area without walking artifact or git object trees.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WorkAreaSnap {
    /// Combined `HEAD` contents and resolved branch ref target across git roots.
    pub head_sig: String,
    /// Max `.git/index` mtime (nanoseconds since UNIX epoch) across git roots.
    pub index_mtime_ns: Option<u128>,
    /// Stable 64-bit hash of `git status --porcelain` when `run_status` was true.
    pub status_hash: Option<u64>,
}

/// Injected work-area reader: inspects git metadata and optionally runs
/// `git status --porcelain` across `roots`.
pub(crate) type WorkAreaRead =
    dyn Fn(&[PathBuf], bool, &mut GitRun<'_>) -> WorkAreaSnap + Send + Sync;

/// Git directories corresponding to a role's work area (never walking trees).
pub(crate) fn role_work_roots(dir: &Path, state: &StudioState, role: &str) -> Vec<PathBuf> {
    let Some(cfg) = state.studio.as_ref() else {
        return Vec::new();
    };
    let base = role_dir(dir, &cfg.repo, role, cfg.no_git);
    if cfg.no_git || cfg.members.is_empty() {
        return vec![base];
    }
    let mut roots = Vec::new();
    let is_worktree = role == "director" || actor_task(role).is_some();
    for member in &cfg.members {
        let p = if is_worktree {
            let leaf = Path::new(member)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| member.clone());
            base.join(leaf)
        } else {
            PathBuf::from(member)
        };
        if !roots.contains(&p) {
            roots.push(p);
        }
    }
    if roots.is_empty() {
        roots.push(base);
    }
    roots
}

/// Inspect `HEAD`, resolved ref, `.git/index` mtime, and (when `run_status` is
/// set) a hash of `git --no-optional-locks status --porcelain` across `roots`.
/// Never walks `target/`, `node_modules/`, or `.git/objects/`.
pub(crate) fn real_work_area(
    roots: &[PathBuf],
    run_status: bool,
    git: &mut GitRun<'_>,
) -> WorkAreaSnap {
    let mut snap = WorkAreaSnap::default();
    let mut status_buf = String::new();
    let mut ran_ok = false;

    for root in roots {
        let Some((git_dir, common_dir)) = resolve_git_dirs(root) else {
            continue;
        };
        let head = fs::read_to_string(git_dir.join("HEAD"))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let ref_val = resolve_head_target(&git_dir, &common_dir, &head);
        snap.head_sig
            .push_str(&format!("{}|{head}|{ref_val}\n", root.display()));

        if run_status {
            let root_s = root.to_string_lossy();
            let args = [
                "--no-optional-locks",
                "-C",
                &root_s,
                "status",
                "--porcelain",
            ];
            let out = git(&args, &root_s);
            if out.code == 0 {
                ran_ok = true;
                status_buf.push_str(&root_s);
                status_buf.push('\n');
                status_buf.push_str(&out.stdout);
                status_buf.push('\0');
            }
        }

        if let Ok(meta) = fs::metadata(git_dir.join("index"))
            && let Ok(mod_t) = meta.modified()
            && let Ok(dur) = mod_t.duration_since(UNIX_EPOCH)
        {
            let ns = dur.as_nanos();
            snap.index_mtime_ns = Some(snap.index_mtime_ns.map_or(ns, |cur| cur.max(ns)));
        }
    }
    if run_status && ran_ok {
        snap.status_hash = Some(fnv1a64(status_buf.as_bytes()));
    }
    snap
}

/// Compare consecutive work-area snapshots.
pub(crate) fn diff_work_area(
    prev: Option<&WorkAreaSnap>,
    prev_status_hash: Option<u64>,
    curr: &WorkAreaSnap,
) -> Option<&'static str> {
    let prev = prev?;
    if !curr.head_sig.is_empty() && curr.head_sig != prev.head_sig {
        return Some("git HEAD changed");
    }
    if curr.index_mtime_ns != prev.index_mtime_ns && curr.index_mtime_ns.is_some() {
        return Some("git index updated");
    }
    if let (Some(old_h), Some(new_h)) = (prev_status_hash, curr.status_hash)
        && new_h != old_h
    {
        return Some("git status changed");
    }
    None
}

fn resolve_git_dirs(root: &Path) -> Option<(PathBuf, PathBuf)> {
    let dot_git = root.join(".git");
    if dot_git.is_dir() {
        let common = read_commondir(&dot_git).unwrap_or_else(|| dot_git.clone());
        return Some((dot_git, common));
    }
    if dot_git.is_file() {
        let text = fs::read_to_string(&dot_git).ok()?;
        let line = text.lines().next()?.trim();
        let raw = line.strip_prefix("gitdir:")?.trim();
        let git_dir = if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            root.join(raw)
        };
        let common = read_commondir(&git_dir).unwrap_or_else(|| git_dir.clone());
        return Some((git_dir, common));
    }
    None
}

fn read_commondir(git_dir: &Path) -> Option<PathBuf> {
    let raw = fs::read_to_string(git_dir.join("commondir")).ok()?;
    let rel = raw.trim();
    if rel.is_empty() {
        return None;
    }
    let p = Path::new(rel);
    Some(if p.is_absolute() {
        p.to_path_buf()
    } else {
        git_dir.join(p)
    })
}

fn resolve_head_target(git_dir: &Path, common_dir: &Path, head: &str) -> String {
    let Some(rel) = head.strip_prefix("ref: ").map(str::trim) else {
        return String::new();
    };
    let rel_path = Path::new(rel);
    if rel_path.is_absolute()
        || rel_path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return String::new();
    }
    for base in [git_dir, common_dir] {
        if let Ok(val) = fs::read_to_string(base.join(rel_path)) {
            return val.trim().to_string();
        }
    }
    if let Ok(packed) = fs::read_to_string(common_dir.join("packed-refs")) {
        for line in packed.lines() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') || t.starts_with('^') {
                continue;
            }
            if let Some((sha, name)) = t.split_once(' ')
                && name == rel
            {
                return sha.to_string();
            }
        }
    }
    String::new()
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

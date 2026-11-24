//! Work-area git probes and delta helpers for [`crate::liveness`].
//! Inspects `HEAD`, resolved branch refs, `.git/index` mtime, and throttled
//! `git status --porcelain` without ever walking `target/`, `node_modules/`, or
//! `.git/objects/`, and diffs `/proc`/`ps` and git snapshots between ticks.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use backlot_model::state::{StudioState, actor_task};

use crate::liveness_probe::{
    ProcInfo, ROOT_CPU_FLOOR_JIFFIES_PER_SEC, ROOT_IO_FLOOR_BYTES_PER_SEC,
};
use crate::workareas::{GitRun, role_dir};

/// Which of the six liveness sources moved (ordered by tie-break priority).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Source {
    Hook,
    Revision,
    Screen,
    Process,
    WorkArea,
    Ledger,
}

impl Source {
    /// Reason recorded on `attention.cleared` when this source ends a stall.
    pub(crate) fn clear_reason(self) -> &'static str {
        match self {
            Source::Hook => "new hook signal",
            Source::Revision => "activity resumed",
            Source::Screen => "screen changed",
            Source::Process => "process activity",
            Source::WorkArea => "work area changed",
            Source::Ledger => "ledger event",
        }
    }
}

/// Details of the latest observed movement for one source or role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceStamp {
    pub at_ms: u64,
    pub desc: String,
    pub tag: &'static str,
}

impl SourceStamp {
    pub(crate) fn new(at_ms: u64, desc: impl Into<String>, tag: &'static str) -> Self {
        Self {
            at_ms,
            desc: desc.into(),
            tag,
        }
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcSnap {
    pub cpu_jiffies: u64,
    pub child_cpu_jiffies: u64,
    pub io_bytes: u64,
    pub argv: String,
    pub wchan: String,
    pub depth: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LockWait {
    pub argv: String,
    pub since_ms: u64,
}

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
/// set) a hash of `git status --porcelain` across `roots`. Never walks
/// `target/`, `node_modules/`, or `.git/objects/`.
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

pub(crate) fn to_proc_map(tree: Vec<ProcInfo>) -> BTreeMap<u32, ProcSnap> {
    tree.into_iter()
        .map(|p| {
            (
                p.pid,
                ProcSnap {
                    cpu_jiffies: p.cpu_jiffies,
                    child_cpu_jiffies: p.child_cpu_jiffies,
                    io_bytes: p.io_bytes,
                    argv: p.argv,
                    wchan: p.wchan,
                    depth: p.depth,
                },
            )
        })
        .collect()
}

pub(crate) fn deepest_descendant(tree: &[ProcInfo]) -> Option<&ProcInfo> {
    tree.iter()
        .filter(|p| p.depth > 0)
        .max_by_key(|p| (p.depth, p.pid))
}

pub(crate) fn update_lock_wait(
    prev: Option<LockWait>,
    tree: &[ProcInfo],
    now_ms: u64,
) -> Option<LockWait> {
    let deep = deepest_descendant(tree)?;
    if !deep.wchan.contains("lock_inode_wait") {
        return None;
    }
    let since_ms = prev
        .filter(|p| p.argv == deep.argv)
        .map_or(now_ms, |p| p.since_ms);
    Some(LockWait {
        argv: deep.argv.clone(),
        since_ms,
    })
}

pub(crate) fn diff_procs(
    prev: &BTreeMap<u32, ProcSnap>,
    curr: &[ProcInfo],
    root_pid: u32,
    dt_ms: u64,
    now_ms: u64,
) -> Option<SourceStamp> {
    let mut best_desc: Option<(u32, u32, SourceStamp)> = None;
    for p in curr {
        if p.depth == 0 {
            continue;
        }
        let stamp = match prev.get(&p.pid) {
            None => Some(SourceStamp {
                at_ms: now_ms,
                desc: p.argv.clone(),
                tag: "child",
            }),
            Some(old) if p.cpu_jiffies > old.cpu_jiffies => Some(SourceStamp {
                at_ms: now_ms,
                desc: p.argv.clone(),
                tag: "CPU",
            }),
            Some(old) if p.io_bytes > old.io_bytes => Some(SourceStamp {
                at_ms: now_ms,
                desc: p.argv.clone(),
                tag: "I/O",
            }),
            Some(old) if p.child_cpu_jiffies > old.child_cpu_jiffies => Some(SourceStamp {
                at_ms: now_ms,
                desc: p.argv.clone(),
                tag: "child exit",
            }),
            Some(_) => None,
        };
        if let Some(st) = stamp
            && best_desc
                .as_ref()
                .is_none_or(|(d, id, _)| (p.depth, p.pid) > (*d, *id))
        {
            best_desc = Some((p.depth, p.pid, st));
        }
    }
    if let Some((_, _, st)) = best_desc {
        return Some(st);
    }

    if let Some(root) = curr.iter().find(|p| p.pid == root_pid)
        && let Some(old_root) = prev.get(&root_pid)
    {
        if root.child_cpu_jiffies > old_root.child_cpu_jiffies {
            return Some(SourceStamp {
                at_ms: now_ms,
                desc: root.argv.clone(),
                tag: "child exit",
            });
        }
        if dt_ms > 0 {
            let cpu_delta = root.cpu_jiffies.saturating_sub(old_root.cpu_jiffies);
            if u128::from(cpu_delta) * 1_000
                > u128::from(ROOT_CPU_FLOOR_JIFFIES_PER_SEC) * u128::from(dt_ms)
            {
                return Some(SourceStamp {
                    at_ms: now_ms,
                    desc: root.argv.clone(),
                    tag: "CPU",
                });
            }
            let io_delta = root.io_bytes.saturating_sub(old_root.io_bytes);
            if u128::from(io_delta) * 1_000
                > u128::from(ROOT_IO_FLOOR_BYTES_PER_SEC) * u128::from(dt_ms)
            {
                return Some(SourceStamp {
                    at_ms: now_ms,
                    desc: root.argv.clone(),
                    tag: "I/O",
                });
            }
        }
    }
    None
}

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

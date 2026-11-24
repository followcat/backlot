//! Process-tree delta evaluation and source stamp types for
//! [`crate::liveness`]. Work-area git probes live in [`crate::liveness_work`].

use std::collections::BTreeMap;

use crate::liveness_probe::{
    ProcInfo, ROOT_CPU_FLOOR_JIFFIES_PER_SEC, ROOT_IO_FLOOR_BYTES_PER_SEC,
};
pub(crate) use crate::liveness_work::{
    WorkAreaRead, WorkAreaSnap, diff_work_area, real_work_area, role_work_roots,
};

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

/// Diff consecutive process trees under `root_pid` to detect process evidence.
///
/// Descendant processes (`depth > 0`) always count on spawn or any CPU, I/O, or
/// waited-child (`cutime + cstime`) growth, and root `child_cpu_jiffies`
/// (`cutime + cstime` from exited children) always counts.
///
/// Root's own CPU and I/O counters (`depth == 0`) count strictly above the
/// `agy`-calibrated floors **only when `!has_hook`** (i.e. `last_signal` is
/// `None` for the current session): TUI runners such as Claude Code (Node/Ink)
/// can burn well above `8 jiffies/s` merely redrawing an animation while stuck,
/// so when hook signals are active they govern the root agent's own turn and
/// root process counters must not outweigh a quiet hook.
pub(crate) fn diff_procs(
    prev: &BTreeMap<u32, ProcSnap>,
    curr: &[ProcInfo],
    root_pid: u32,
    dt_ms: u64,
    now_ms: u64,
    has_hook: bool,
) -> Option<SourceStamp> {
    let mut best_desc: Option<(u32, u32, SourceStamp)> = None;
    for p in curr {
        if p.depth == 0 {
            continue;
        }
        let stamp = match prev.get(&p.pid) {
            None => Some(SourceStamp::new(now_ms, &p.argv, "child")),
            Some(old) if p.cpu_jiffies > old.cpu_jiffies => {
                Some(SourceStamp::new(now_ms, &p.argv, "CPU"))
            }
            Some(old) if p.io_bytes > old.io_bytes => {
                Some(SourceStamp::new(now_ms, &p.argv, "I/O"))
            }
            Some(old) if p.child_cpu_jiffies > old.child_cpu_jiffies => {
                Some(SourceStamp::new(now_ms, &p.argv, "child exit"))
            }
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
            return Some(SourceStamp::new(now_ms, &root.argv, "child exit"));
        }
        if !has_hook && dt_ms > 0 {
            let cpu_delta = root.cpu_jiffies.saturating_sub(old_root.cpu_jiffies);
            if u128::from(cpu_delta) * 1_000
                > u128::from(ROOT_CPU_FLOOR_JIFFIES_PER_SEC) * u128::from(dt_ms)
            {
                return Some(SourceStamp::new(now_ms, &root.argv, "CPU"));
            }
            let io_delta = root.io_bytes.saturating_sub(old_root.io_bytes);
            if u128::from(io_delta) * 1_000
                > u128::from(ROOT_IO_FLOOR_BYTES_PER_SEC) * u128::from(dt_ms)
            {
                return Some(SourceStamp::new(now_ms, &root.argv, "I/O"));
            }
        }
    }
    None
}

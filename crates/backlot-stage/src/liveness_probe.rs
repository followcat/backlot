//! OS process-tree probes for liveness evidence (D4): walks `/proc` on
//! Linux and `ps` on macOS from a terminal's root PID without touching
//! unrelated processes.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::Path;

/// Safety bound on descendant traversal so a fork burst cannot stall a tick.
const MAX_PROCS: usize = 512;

/// Character cap on a recorded `argv` line in `detail`.
const MAX_ARGV_CHARS: usize = 120;

/// Root-agent CPU floor above which root process CPU counts as movement.
/// Derived from R7a measurements of `agy 1.5.16`: idling/waiting held at
/// `2-4 jiffies/s` while active streaming reached `~15.6 jiffies/s` (`+218`
/// jiffies over 14 s); `8 jiffies/s` sits at 2x max idle noise and half the
/// streaming rate.
pub(crate) const ROOT_CPU_FLOOR_JIFFIES_PER_SEC: u64 = 8;

/// Root-agent `rchar+wchar` I/O floor above which root I/O counts as movement.
/// Derived from R7a measurements of `agy 1.5.16`: idling/waiting held at
/// `1-7 KB/s` (stdio/heartbeat polling) while streaming wrote `~500 KB/s`
/// (`+6.84 MB` over 14 s); `16 KB/s` sits above 2x max idle noise and well
/// below real streaming I/O.
pub(crate) const ROOT_IO_FLOOR_BYTES_PER_SEC: u64 = 16 * 1024;

/// One live process snapshot under a role terminal's root PID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcInfo {
    pub pid: u32,
    pub ppid: u32,
    pub depth: u32,
    /// `utime + stime` in jiffies (100Hz centiseconds on macOS `ps`).
    pub cpu_jiffies: u64,
    /// `cutime + cstime` — cumulative CPU jiffies of waited exited children.
    pub child_cpu_jiffies: u64,
    /// Cumulative `rchar + wchar` bytes from `/proc/<pid>/io`.
    pub io_bytes: u64,
    /// Joined `cmdline` arguments (or `comm` fallback), capped for one-line detail.
    pub argv: String,
    /// Kernel wait channel from `/proc/<pid>/wchan` (empty when unset or macOS).
    pub wchan: String,
}

/// Injected process-tree reader: given `term.pid`, returns processes from root
/// (`depth == 0`) down to descendants.
pub(crate) type ProcRead = dyn Fn(u32) -> Vec<ProcInfo> + Send + Sync;

/// Production process-tree probe on the local host.
pub(crate) fn real_proc_tree(root_pid: u32) -> Vec<ProcInfo> {
    if root_pid == 0 {
        return Vec::new();
    }
    #[cfg(target_os = "linux")]
    {
        read_proc_tree_from(Path::new("/proc"), root_pid)
    }
    #[cfg(target_os = "macos")]
    {
        let Ok(out) = std::process::Command::new("ps")
            .args(["-axo", "pid=,ppid=,time=,command="])
            .output()
        else {
            return Vec::new();
        };
        if !out.status.success() {
            return Vec::new();
        }
        parse_macos_ps(root_pid, &String::from_utf8_lossy(&out.stdout))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Vec::new()
    }
}

/// Walk a Linux `/proc`-shaped tree from `root_pid` using
/// `/proc/<pid>/task/*/children`, falling back to scanning `ppid` across
/// `/proc/*/stat` if `children` files are absent.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn read_proc_tree_from(proc_root: &Path, root_pid: u32) -> Vec<ProcInfo> {
    if root_pid == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::from([(root_pid, 0u32)]);
    let mut saw_children_file = false;
    let mut ppid_fallback: Option<BTreeMap<u32, Vec<u32>>> = None;

    while let Some((pid, depth)) = queue.pop_front() {
        if !visited.insert(pid) {
            continue;
        }
        if out.len() >= MAX_PROCS {
            break;
        }
        let pdir = proc_root.join(pid.to_string());
        let Ok(stat_raw) = fs::read_to_string(pdir.join("stat")) else {
            continue;
        };
        let Some(stat) = parse_proc_stat(&stat_raw) else {
            continue;
        };
        let io_bytes = fs::read_to_string(pdir.join("io"))
            .map(|s| parse_proc_io(&s))
            .unwrap_or(0);
        let argv = fs::read(pdir.join("cmdline"))
            .map(|b| parse_proc_cmdline(&b, &stat.comm))
            .unwrap_or_else(|_| truncate_argv(&stat.comm));
        let wchan = fs::read_to_string(pdir.join("wchan"))
            .map(|s| {
                let t = s.trim();
                if t == "0" {
                    String::new()
                } else {
                    t.to_string()
                }
            })
            .unwrap_or_default();

        out.push(ProcInfo {
            pid,
            ppid: stat.ppid,
            depth,
            cpu_jiffies: stat.cpu_jiffies,
            child_cpu_jiffies: stat.child_cpu_jiffies,
            io_bytes,
            argv,
            wchan,
        });

        let (kids, has_files) = read_task_children(&pdir.join("task"));
        if has_files {
            saw_children_file = true;
            for cpid in kids {
                if !visited.contains(&cpid) {
                    queue.push_back((cpid, depth + 1));
                }
            }
        } else if !saw_children_file {
            let map = ppid_fallback.get_or_insert_with(|| scan_ppid_map(proc_root));
            if let Some(cpids) = map.get(&pid) {
                for &cpid in cpids {
                    if !visited.contains(&cpid) {
                        queue.push_back((cpid, depth + 1));
                    }
                }
            }
        }
    }
    out
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
struct ParsedStat {
    ppid: u32,
    comm: String,
    cpu_jiffies: u64,
    child_cpu_jiffies: u64,
}

/// Parse `/proc/<pid>/stat`, locating the last `')'` so process names with
/// spaces or parentheses never shift fields.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_proc_stat(raw: &str) -> Option<ParsedStat> {
    let lp = raw.find('(')?;
    let rp = raw.rfind(')')?;
    if rp <= lp {
        return None;
    }
    let comm = raw[lp + 1..rp].to_string();
    let fields: Vec<&str> = raw[rp + 1..].split_whitespace().collect();
    // Fields after `)`: [0]=state, [1]=ppid, [11]=utime, [12]=stime,
    // [13]=cutime, [14]=cstime.
    let ppid = fields.get(1)?.parse::<u32>().ok()?;
    let utime = fields
        .get(11)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let stime = fields
        .get(12)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let cutime = fields
        .get(13)
        .and_then(|s| s.parse::<i64>().ok())
        .map(|v| v.max(0) as u64)
        .unwrap_or(0);
    let cstime = fields
        .get(14)
        .and_then(|s| s.parse::<i64>().ok())
        .map(|v| v.max(0) as u64)
        .unwrap_or(0);
    Some(ParsedStat {
        ppid,
        comm,
        cpu_jiffies: utime.saturating_add(stime),
        child_cpu_jiffies: cutime.saturating_add(cstime),
    })
}

/// Sum `rchar` and `wchar` from `/proc/<pid>/io`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_proc_io(raw: &str) -> u64 {
    let mut total = 0u64;
    for line in raw.lines() {
        if let Some(rest) = line
            .strip_prefix("rchar:")
            .or_else(|| line.strip_prefix("wchar:"))
            && let Ok(n) = rest.trim().parse::<u64>()
        {
            total = total.saturating_add(n);
        }
    }
    total
}

/// Join NUL-separated `/proc/<pid>/cmdline` tokens, falling back to `comm`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_proc_cmdline(raw: &[u8], comm: &str) -> String {
    let mut parts = Vec::new();
    for seg in raw.split(|&b| b == 0) {
        if !seg.is_empty() {
            parts.push(String::from_utf8_lossy(seg).into_owned());
        }
    }
    let joined = parts.join(" ");
    let base = if joined.trim().is_empty() {
        comm.trim()
    } else {
        joined.trim()
    };
    truncate_argv(base)
}

fn truncate_argv(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= MAX_ARGV_CHARS {
        return trimmed.to_string();
    }
    let prefix: String = trimmed.chars().take(MAX_ARGV_CHARS - 1).collect();
    format!("{prefix}…")
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn read_task_children(task_dir: &Path) -> (Vec<u32>, bool) {
    let Ok(entries) = fs::read_dir(task_dir) else {
        return (Vec::new(), false);
    };
    let mut kids = Vec::new();
    let mut seen = BTreeSet::new();
    let mut has_files = false;
    for entry in entries.flatten() {
        if let Ok(content) = fs::read_to_string(entry.path().join("children")) {
            has_files = true;
            for tok in content.split_whitespace() {
                if let Ok(pid) = tok.parse::<u32>()
                    && seen.insert(pid)
                {
                    kids.push(pid);
                }
            }
        }
    }
    (kids, has_files)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn scan_ppid_map(proc_root: &Path) -> BTreeMap<u32, Vec<u32>> {
    let mut map: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let Ok(entries) = fs::read_dir(proc_root) else {
        return map;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(s) = name.to_str() else {
            continue;
        };
        let Ok(pid) = s.parse::<u32>() else {
            continue;
        };
        let Ok(raw) = fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        if let Some(parsed) = parse_proc_stat(&raw) {
            map.entry(parsed.ppid).or_default().push(pid);
        }
    }
    map
}

/// Parse `ps -axo pid=,ppid=,time=,command=` output into the tree rooted at
/// `root_pid` (CPU-only: `io_bytes` and `wchan` are empty on macOS).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn parse_macos_ps(root_pid: u32, raw: &str) -> Vec<ProcInfo> {
    let mut by_pid: BTreeMap<u32, ProcInfo> = BTreeMap::new();
    let mut by_ppid: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for line in raw.lines() {
        let mut it = line.split_whitespace();
        let Some(Ok(pid)) = it.next().map(str::parse::<u32>) else {
            continue;
        };
        let Some(Ok(ppid)) = it.next().map(str::parse::<u32>) else {
            continue;
        };
        let Some(time_str) = it.next() else {
            continue;
        };
        let cmd: Vec<&str> = it.collect();
        let argv = truncate_argv(&cmd.join(" "));
        let cpu_jiffies = parse_ps_time_jiffies(time_str);
        by_pid.insert(
            pid,
            ProcInfo {
                pid,
                ppid,
                depth: 0,
                cpu_jiffies,
                child_cpu_jiffies: 0,
                io_bytes: 0,
                argv,
                wchan: String::new(),
            },
        );
        by_ppid.entry(ppid).or_default().push(pid);
    }
    let mut out = Vec::new();
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::from([(root_pid, 0u32)]);
    while let Some((pid, depth)) = queue.pop_front() {
        if !visited.insert(pid) || out.len() >= MAX_PROCS {
            continue;
        }
        if let Some(mut info) = by_pid.remove(&pid) {
            info.depth = depth;
            out.push(info);
            if let Some(kids) = by_ppid.get(&pid) {
                for &k in kids {
                    if !visited.contains(&k) {
                        queue.push_back((k, depth + 1));
                    }
                }
            }
        }
    }
    out
}

/// Convert macOS `ps` elapsed CPU `[[D-]HH:]MM:SS[.cc]` to 100Hz units.
fn parse_ps_time_jiffies(s: &str) -> u64 {
    let (days, rest) = match s.split_once('-') {
        Some((d, r)) => (d.parse::<u64>().unwrap_or(0), r),
        None => (0, s),
    };
    let (hms, frac) = match rest.split_once('.') {
        Some((t, f)) => {
            let two: String = f.chars().take(2).collect();
            let val = two.parse::<u64>().unwrap_or(0);
            let centis = if two.len() == 1 { val * 10 } else { val };
            (t, centis)
        }
        None => (rest, 0),
    };
    let parts: Vec<u64> = hms
        .split(':')
        .filter_map(|p| p.parse::<u64>().ok())
        .collect();
    let secs = match parts.as_slice() {
        [m, s] => m * 60 + s,
        [h, m, s] => h * 3600 + m * 60 + s,
        [s] => *s,
        _ => 0,
    };
    (days * 86_400 + secs) * 100 + frac
}

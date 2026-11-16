//! The process-tree layer: parse `ps -A -o pid=,ppid=,etime=,args=`
//! and find the tool shells an agent's descendants still run. Pure —
//! the probe worker feeds it the `machine.exec` stdout.
//!
//! The command line is `ps`'s portable field set on Linux and macOS:
//! `argv` runs unshelled so `ps` itself needs no shell.

use std::collections::{HashMap, HashSet};

/// `machine.exec`'s argument list for the listing: `args=` keeps the
/// full command line the `-c`-form check needs.
pub(crate) const PS_ARGV: &[&str] = &["ps", "-A", "-o", "pid=,ppid=,etime=,args="];

/// Basenames that count as shells: the tool shells every agent spawns
/// for `Run command`-style work. Long-lived helpers (`node`, `npm
/// exec`, `uvx`, language servers) are not shells and never count.
const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "fish", "ksh"];

/// A command-line flag that hands the shell a command string: `-c`,
/// or a short-option cluster ending in `c` whose letters are the
/// no-argument shell flags `l`/`i`/`x`/`s` (`-lc`, `-ic`, `-lic`,
/// `-xc`). Long options and lookalikes (`--command`, `-exec`) are not
/// command strings.
fn is_c_form(arg: &str) -> bool {
    let Some(body) = arg.strip_prefix('-').filter(|body| !body.starts_with('-')) else {
        return false;
    };
    body.strip_suffix('c')
        .is_some_and(|flags| flags.chars().all(|c| matches!(c, 'l' | 'i' | 'x' | 's')))
}

/// A tool shell: the executable's basename is a shell (a `-` login
/// prefix on argv[0] is ignored) and an argument is a `-c` form.
pub(crate) fn tool_shell(argv: &[String]) -> bool {
    let Some(exe) = argv.first() else {
        return false;
    };
    let base = exe.rsplit('/').next().unwrap_or(exe);
    SHELLS.contains(&base.trim_start_matches('-')) && argv[1..].iter().any(|a| is_c_form(a))
}

/// `ps`'s `etime` field: `[[dd-]hh:]mm:ss`.
pub(crate) fn parse_etime(word: &str) -> Option<u64> {
    let (days, rest) = match word.split_once('-') {
        Some((days, rest)) => (days.parse::<u64>().ok()?, rest),
        None => (0, word),
    };
    let mut parts = rest.split(':');
    let (hours, minutes, seconds) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(minutes), Some(seconds), None, None) => (
            0,
            minutes.parse::<u64>().ok()?,
            seconds.parse::<u64>().ok()?,
        ),
        (Some(hours), Some(minutes), Some(seconds), None) => (
            hours.parse::<u64>().ok()?,
            minutes.parse::<u64>().ok()?,
            seconds.parse::<u64>().ok()?,
        ),
        _ => return None,
    };
    Some(days * 86_400 + hours * 3_600 + minutes * 60 + seconds)
}

/// One parsed `ps` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessRow {
    pub pid: u32,
    pub ppid: u32,
    /// Seconds since the process started.
    pub etime: u64,
    pub argv: Vec<String>,
}

/// The whole listing, one `pid ppid etime args` tuple per line;
/// unparseable lines are skipped (a process could exit mid-read).
pub(crate) fn parse_processes(text: &str) -> Vec<ProcessRow> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            Some(ProcessRow {
                pid: words.next()?.parse().ok()?,
                ppid: words.next()?.parse().ok()?,
                etime: parse_etime(words.next()?)?,
                argv: words.map(str::to_owned).collect(),
            })
        })
        .collect()
}

/// Whether `agent_pid` still runs a tool shell: a live descendant at
/// any depth whose argv matches `tool_shell`, started at least
/// `window_secs` after the agent itself (younger shells are startup
/// helpers). The agent's own row supplies the reference etime; a
/// missing row yields no evidence.
pub(crate) fn has_tool_shell_descendant(
    rows: &[ProcessRow],
    agent_pid: u32,
    window_secs: u64,
) -> bool {
    let Some(agent_etime) = rows
        .iter()
        .find(|row| row.pid == agent_pid)
        .map(|r| r.etime)
    else {
        return false;
    };
    let mut children_of: HashMap<u32, Vec<&ProcessRow>> = HashMap::new();
    for row in rows {
        children_of.entry(row.ppid).or_default().push(row);
    }
    let mut seen = HashSet::from([agent_pid]);
    let mut frontier: Vec<&ProcessRow> = children_of.get(&agent_pid).cloned().unwrap_or_default();
    while let Some(row) = frontier.pop() {
        if !seen.insert(row.pid) {
            continue;
        }
        // Started after the agent (a descendant must be), outside the
        // start window, and a tool shell — that's the evidence.
        if agent_etime.saturating_sub(row.etime) >= window_secs && tool_shell(&row.argv) {
            return true;
        }
        if let Some(children) = children_of.get(&row.pid) {
            frontier.extend(children);
        }
    }
    false
}

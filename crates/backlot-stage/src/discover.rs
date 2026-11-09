//! Finding what a runner only reveals after launch: the session id
//! codex leaves in `~/.codex/sessions` and the one `devin list`
//! reports, plus the `<runner> --version` line `role.started` carries.
//! Every external command goes through the injected [`Exec`], so the
//! stage never names a CLI in a test and tests never spawn one.

use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::runners;

/// One external command the stage wants run.
pub struct ExecCall {
    pub argv: Vec<String>,
    /// The role's working directory.
    pub cwd: String,
    pub timeout: Duration,
}

/// How the stage runs external commands: argv and cwd in, stdout or
/// the failure string out. Injected so tests never spawn a real CLI.
pub type Exec = dyn Fn(&ExecCall) -> Result<String, String> + Send + Sync;

/// The real [`Exec`]: spawn, drain stdout on its own thread into a
/// shared buffer — a child writing more than the pipe holds would
/// block forever with nobody reading — then poll and kill past the
/// timeout. The join is never unbounded: a grandchild that keeps
/// the pipe open (a runner's background service) would otherwise
/// stop the stage loop with it. After the child exits the reader
/// gets a short grace to reach end of file, and whatever was read
/// is the answer; after a timeout kill the reader is left running —
/// it ends when the pipe finally closes.
pub fn real_exec(call: &ExecCall) -> Result<String, String> {
    use std::io::Read;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    let Some((program, args)) = call.argv.split_first() else {
        return Err("empty argv".into());
    };
    let mut child = std::process::Command::new(program)
        .args(args)
        .current_dir(&call.cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    let stdout = child.stdout.take();
    let buf = Arc::new(Mutex::new(Vec::new()));
    let done = Arc::new(AtomicBool::new(false));
    let reader = {
        let buf = buf.clone();
        let done = done.clone();
        std::thread::spawn(move || {
            if let Some(mut pipe) = stdout {
                let mut chunk = [0u8; 8192];
                loop {
                    match pipe.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.lock().unwrap().extend_from_slice(&chunk[..n]),
                    }
                }
            }
            done.store(true, Ordering::Relaxed);
        })
    };
    let _ = reader; // never joined — see the doc comment
    let deadline = std::time::Instant::now() + call.timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let grace = std::time::Instant::now() + Duration::from_millis(200);
                while !done.load(Ordering::Relaxed) && std::time::Instant::now() < grace {
                    std::thread::sleep(Duration::from_millis(10));
                }
                let out = std::mem::take(&mut *buf.lock().unwrap());
                return Ok(String::from_utf8_lossy(&out).into_owned());
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program}: timed out"));
            }
            Err(e) => return Err(format!("{program}: {e}")),
        }
    }
}

/// `<runner> --version`'s first line, capped at five seconds — a
/// failure or an empty one leaves `runner_version` empty.
pub fn runner_version(exec: &Exec, runner: &str) -> Option<String> {
    let call = ExecCall {
        argv: vec![runner.to_string(), "--version".into()],
        cwd: ".".into(),
        timeout: Duration::from_secs(5),
    };
    exec(&call)
        .ok()
        .and_then(|out| out.lines().next().map(str::trim).map(str::to_string))
        .filter(|line| !line.is_empty())
}

/// The session id a live role's runner assigned, if it can be found:
/// chosen ids read back off the terminal's argv (an adopted claude or
/// opencode), the codex rollout tree or `devin list`. `taken` holds
/// the session ids other roles in the studio already claim.
pub fn find_session(
    runner: &str,
    argv: &[String],
    cwd: &str,
    started_ms: u64,
    taken: &BTreeSet<String>,
    home: &Path,
    exec: &Exec,
) -> Option<String> {
    match runner {
        "claude" | "opencode" => {
            runners::session_in_argv(runner, argv).filter(|id| !taken.contains(id))
        }
        "codex" => codex_session(home, cwd, started_ms, taken),
        "devin" => devin_session(exec, cwd, started_ms, taken),
        _ => None,
    }
}

/// A file's birth time, or its mtime where birth time is unsupported.
fn file_time_ms(path: &Path) -> Option<u64> {
    let md = fs::metadata(path).ok()?;
    let t = md.created().or_else(|_| md.modified()).ok()?;
    Some(
        t.duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    )
}

/// `YYYY/MM/DD` for a unix-ms timestamp in UTC — Howard Hinnant's
/// civil-from-days, so no date dependency is needed.
fn day_string(ms: u64) -> String {
    let z = (ms / 86_400_000) as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}/{m:02}/{d:02}")
}

/// Day directories `YYYY/MM/DD` under `base` at or after `floor`
/// (same shape), sorted. The walk prunes by segment — an older
/// year, month or day directory is never even opened, and names
/// that are not date segments are skipped.
fn collect_days(base: &Path, floor: &str) -> Vec<PathBuf> {
    let bounds: Vec<&str> = floor.split('/').collect();
    let mut out = Vec::new();
    if bounds.len() == 3 {
        walk_days(base, 0, &bounds, true, &mut out);
    }
    out.sort();
    out
}

fn walk_days(dir: &Path, level: usize, bounds: &[&str], on_floor: bool, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.filter_map(|e| e.ok()) {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.len() != [4, 2, 2][level] || !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if on_floor && name < bounds[level] {
            continue;
        }
        if level == 2 {
            out.push(p);
        } else {
            walk_days(
                &p,
                level + 1,
                bounds,
                on_floor && name == bounds[level],
                out,
            );
        }
    }
}

/// Codex's session files, oldest match first: day directories and
/// their `rollout-*.jsonl` names sort chronologically, so the first
/// candidate is the session the launch created — a person's own
/// session opened later in the same folder never wins. Only day
/// directories from the day before the role started onward are
/// read at all. A file counts when its cwd is the role's, it was
/// created after the role started and no other role already claims
/// the id.
fn codex_session(
    home: &Path,
    cwd: &str,
    started_ms: u64,
    taken: &BTreeSet<String>,
) -> Option<String> {
    let sessions = home.join(".codex").join("sessions");
    let floor = day_string(started_ms.saturating_sub(86_400_000));
    for day in collect_days(&sessions, &floor) {
        let Ok(entries) = fs::read_dir(&day) else {
            continue;
        };
        let mut rollouts: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("rollout-") && n.ends_with(".jsonl"))
            })
            .collect();
        rollouts.sort();
        for f in rollouts {
            if file_time_ms(&f).is_none_or(|ms| ms < started_ms) {
                continue;
            }
            let Ok(file) = fs::File::open(&f) else {
                continue;
            };
            let mut first = String::new();
            if BufReader::new(file).read_line(&mut first).is_err() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&first) else {
                continue;
            };
            let (Some(id), Some(file_cwd)) =
                (v["payload"]["id"].as_str(), v["payload"]["cwd"].as_str())
            else {
                continue;
            };
            if file_cwd == cwd && !taken.contains(id) {
                return Some(id.to_string());
            }
        }
    }
    None
}

/// `devin list --format json` in the role's cwd: an array of
/// `{"id", "last_activity_at"}` (seconds). The oldest entry active
/// at or after the role's start, unclaimed elsewhere, is the
/// session — a session a person opened later in the same folder
/// never wins.
fn devin_session(
    exec: &Exec,
    cwd: &str,
    started_ms: u64,
    taken: &BTreeSet<String>,
) -> Option<String> {
    let call = ExecCall {
        argv: vec![
            "devin".into(),
            "list".into(),
            "--format".into(),
            "json".into(),
        ],
        cwd: cwd.to_string(),
        timeout: Duration::from_secs(10),
    };
    let out = exec(&call).ok()?;
    let entries: Vec<serde_json::Value> = serde_json::from_str(&out).ok()?;
    let started_s = started_ms / 1000;
    let mut best: Option<(u64, &str)> = None;
    for e in &entries {
        let (Some(id), Some(at)) = (e["id"].as_str(), e["last_activity_at"].as_u64()) else {
            continue;
        };
        if at < started_s || taken.contains(id) {
            continue;
        }
        if best.is_none_or(|(b, _)| at < b) {
            best = Some((at, id));
        }
    }
    best.map(|(_, id)| id.to_string())
}

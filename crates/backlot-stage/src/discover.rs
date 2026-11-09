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

/// The real [`Exec`]: spawn, poll, kill past the timeout.
pub fn real_exec(call: &ExecCall) -> Result<String, String> {
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
    let deadline = std::time::Instant::now() + call.timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let mut out = String::new();
                if let Some(mut pipe) = child.stdout.take() {
                    use std::io::Read;
                    let _ = pipe.read_to_string(&mut out);
                }
                return Ok(out);
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

/// Codex's session files, newest day directory first: the first line
/// of each `rollout-*.jsonl` is `{"payload": {"id", "cwd", ...}}`. A
/// file counts when its cwd is the role's, it was created after the
/// role started and no other role already claims the id.
fn codex_session(
    home: &Path,
    cwd: &str,
    started_ms: u64,
    taken: &BTreeSet<String>,
) -> Option<String> {
    let mut days: Vec<PathBuf> = Vec::new();
    let sessions = home.join(".codex").join("sessions");
    collect_dirs(&sessions, 3, &mut days);
    days.sort();
    days.reverse();
    for day in days {
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
        rollouts.reverse();
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

/// Collect every directory exactly `depth` levels under `base`.
fn collect_dirs(base: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(base) else {
        return;
    };
    for e in entries.filter_map(|e| e.ok()) {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        if depth == 1 {
            out.push(p);
        } else {
            collect_dirs(&p, depth - 1, out);
        }
    }
}

/// `devin list --format json` in the role's cwd: an array of
/// `{"id", "last_activity_at"}` (seconds). The newest entry started
/// at or after the role, unclaimed elsewhere, is the session.
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
        if best.is_none_or(|(b, _)| at > b) {
            best = Some((at, id));
        }
    }
    best.map(|(_, id)| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    const CWD: &str = "/home/user/acme-shop";

    /// A fake `~/.codex/sessions/<y>/<m>/<d>/rollout-<n>.jsonl` tree.
    fn rollout(home: &Path, day: &str, name: &str, id: &str, cwd: &str) {
        let dir = home.join(".codex/sessions").join(day);
        fs::create_dir_all(&dir).unwrap();
        let first = format!("{{\"payload\":{{\"id\":\"{id}\",\"cwd\":\"{cwd}\"}}}}\n");
        fs::write(dir.join(name), first).unwrap();
    }

    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bl-discover-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn no_exec(_: &ExecCall) -> Result<String, String> {
        panic!("tests never spawn")
    }

    /// A rollout from this run, in the role's cwd, is the session.
    #[test]
    fn codex_finds_its_rollout() {
        let home = tmp("codex-ok");
        rollout(&home, "2026/11/09", "rollout-a.jsonl", "sess-1", CWD);
        let taken = BTreeSet::new();
        assert_eq!(
            find_session("codex", &[], CWD, 1, &taken, &home, &no_exec).as_deref(),
            Some("sess-1")
        );
    }

    /// The newest day is scanned first; a wrong cwd or an already
    /// claimed id does not count, and a pre-start file never does.
    #[test]
    fn codex_rejects_the_wrong_files() {
        let home = tmp("codex-reject");
        // Wrong cwd on the newest day, the right one a day older.
        rollout(
            &home,
            "2026/11/10",
            "rollout-b.jsonl",
            "sess-new",
            "/elsewhere",
        );
        rollout(&home, "2026/11/09", "rollout-a.jsonl", "sess-old", CWD);
        let taken = BTreeSet::new();
        assert_eq!(
            find_session("codex", &[], CWD, 1, &taken, &home, &no_exec).as_deref(),
            Some("sess-old"),
            "the scan keeps going past a wrong-cwd file"
        );
        // An id another role claims is not this role's.
        let taken = BTreeSet::from(["sess-old".to_string()]);
        assert_eq!(
            find_session("codex", &[], CWD, 1, &taken, &home, &no_exec),
            None
        );
        // A start far in the future makes every file pre-start.
        let far = u64::MAX;
        assert_eq!(
            find_session("codex", &[], CWD, far, &BTreeSet::new(), &home, &no_exec),
            None
        );
        // And a tree that is not there at all finds nothing.
        let empty = tmp("codex-empty");
        assert_eq!(
            find_session("codex", &[], CWD, 1, &BTreeSet::new(), &empty, &no_exec),
            None
        );
    }

    /// `devin list` runs in the role's cwd; the freshest entry after
    /// the role started wins, claims and older entries lose.
    #[test]
    fn devin_finds_its_entry() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        let exec = move |call: &ExecCall| {
            seen.lock()
                .unwrap()
                .push((call.argv.clone(), call.cwd.clone()));
            Ok(r#"[
                {"id": "dev-old", "last_activity_at": 90},
                {"id": "dev-taken", "last_activity_at": 150},
                {"id": "dev-new", "last_activity_at": 120}
            ]"#
            .to_string())
        };
        let taken = BTreeSet::from(["dev-taken".to_string()]);
        // Role started at 100s: dev-old is pre-start, dev-taken is
        // claimed, dev-new wins.
        assert_eq!(
            find_session("devin", &[], CWD, 100_000, &taken, Path::new("/h"), &exec).as_deref(),
            Some("dev-new")
        );
        let calls = calls.lock().unwrap();
        assert_eq!(
            calls[0].0,
            ["devin", "list", "--format", "json"]
                .map(String::from)
                .to_vec()
        );
        assert_eq!(calls[0].1, CWD);
    }

    /// A failing or malformed `devin list` finds nothing.
    #[test]
    fn devin_failures_find_nothing() {
        let failing = |_: &ExecCall| Err("no devin".to_string());
        assert_eq!(
            find_session(
                "devin",
                &[],
                CWD,
                1,
                &BTreeSet::new(),
                Path::new("/h"),
                &failing
            ),
            None
        );
        let garbage = |_: &ExecCall| Ok("not json".to_string());
        assert_eq!(
            find_session(
                "devin",
                &[],
                CWD,
                1,
                &BTreeSet::new(),
                Path::new("/h"),
                &garbage
            ),
            None
        );
    }

    /// Chosen ids read back off argv respect `taken`; runners that
    /// do not choose give nothing here.
    #[test]
    fn chosen_ids_come_off_argv() {
        let argv = vec![
            "claude".to_string(),
            "--session-id".to_string(),
            "uuid-1".to_string(),
        ];
        let taken = BTreeSet::new();
        assert_eq!(
            find_session("claude", &argv, CWD, 0, &taken, Path::new("/h"), &no_exec).as_deref(),
            Some("uuid-1")
        );
        let taken = BTreeSet::from(["uuid-1".to_string()]);
        assert_eq!(
            find_session("claude", &argv, CWD, 0, &taken, Path::new("/h"), &no_exec),
            None
        );
        assert_eq!(
            find_session(
                "fake",
                &argv,
                CWD,
                0,
                &BTreeSet::new(),
                Path::new("/h"),
                &no_exec
            ),
            None
        );
    }

    /// The version probe takes the first line; failure and empty
    /// output leave it unset.
    #[test]
    fn version_is_the_first_line_or_nothing() {
        let exec = |call: &ExecCall| {
            assert_eq!(call.argv, ["devin".to_string(), "--version".to_string()]);
            Ok("devin 3000.12.32\nmore\n".to_string())
        };
        assert_eq!(
            runner_version(&exec, "devin").as_deref(),
            Some("devin 3000.12.32")
        );
        let failing = |_: &ExecCall| Err("gone".to_string());
        assert_eq!(runner_version(&failing, "gone"), None);
        let empty = |_: &ExecCall| Ok("\n".to_string());
        assert_eq!(runner_version(&empty, "quiet"), None);
    }
}

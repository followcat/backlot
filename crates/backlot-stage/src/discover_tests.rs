//! Tests for `discover`: fake rollout trees and injected exec output —
//! nothing here spawns a real CLI.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::discover::{ExecCall, Look, find_session, real_exec, runner_version};

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
    let role_dir = Path::new("/rd");
    let taken = BTreeSet::new();
    assert_eq!(
        find_session(
            "codex",
            &[],
            CWD,
            1,
            &taken,
            &Look {
                home: &home,
                role_dir
            },
            &no_exec
        )
        .as_deref(),
        Some("sess-1")
    );
}

/// The oldest matching rollout wins: a person's own codex session
/// opened later in the same folder — a newer day directory, a later
/// filename — can never take a role's id.
#[test]
fn codex_prefers_the_oldest_match() {
    let home = tmp("codex-oldest");
    // The role's file first, a person's later one second — the
    // names sort the same way they were made.
    rollout(&home, "2026/11/09", "rollout-a.jsonl", "sess-role", CWD);
    rollout(&home, "2026/11/09", "rollout-z.jsonl", "sess-person", CWD);
    rollout(&home, "2026/11/10", "rollout-b.jsonl", "sess-next-day", CWD);
    let role_dir = Path::new("/rd");
    let taken = BTreeSet::new();
    assert_eq!(
        find_session(
            "codex",
            &[],
            CWD,
            1,
            &taken,
            &Look {
                home: &home,
                role_dir
            },
            &no_exec
        )
        .as_deref(),
        Some("sess-role"),
        "the earliest rollout in the folder is the role's own"
    );
    // With the role's id already claimed, the next-oldest serves.
    let taken = BTreeSet::from(["sess-role".to_string()]);
    assert_eq!(
        find_session(
            "codex",
            &[],
            CWD,
            1,
            &taken,
            &Look {
                home: &home,
                role_dir
            },
            &no_exec
        )
        .as_deref(),
        Some("sess-person")
    );
}

/// A day directory older than the day before the role started is
/// never read — its files are not even opened.
#[test]
fn codex_skips_day_directories_before_the_role() {
    let home = tmp("codex-days");
    rollout(&home, "1970/01/01", "rollout-a.jsonl", "sess-ancient", CWD);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let role_dir = Path::new("/rd");
    // The file itself is fresh — only its day directory is stale.
    assert_eq!(
        find_session(
            "codex",
            &[],
            CWD,
            now.saturating_sub(60_000),
            &BTreeSet::new(),
            &Look {
                home: &home,
                role_dir
            },
            &no_exec
        ),
        None,
        "a day directory from before the role started is never read"
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
    let role_dir = Path::new("/rd");
    let taken = BTreeSet::new();
    assert_eq!(
        find_session(
            "codex",
            &[],
            CWD,
            1,
            &taken,
            &Look {
                home: &home,
                role_dir
            },
            &no_exec
        )
        .as_deref(),
        Some("sess-old"),
        "the scan keeps going past a wrong-cwd file"
    );
    // An id another role claims is not this role's.
    let taken = BTreeSet::from(["sess-old".to_string()]);
    assert_eq!(
        find_session(
            "codex",
            &[],
            CWD,
            1,
            &taken,
            &Look {
                home: &home,
                role_dir
            },
            &no_exec
        ),
        None
    );
    // A start far in the future makes every file pre-start.
    let far = u64::MAX;
    assert_eq!(
        find_session(
            "codex",
            &[],
            CWD,
            far,
            &BTreeSet::new(),
            &Look {
                home: &home,
                role_dir
            },
            &no_exec
        ),
        None
    );
    // And a tree that is not there at all finds nothing.
    let empty = tmp("codex-empty");
    assert_eq!(
        find_session(
            "codex",
            &[],
            CWD,
            1,
            &BTreeSet::new(),
            &Look {
                home: &empty,
                role_dir
            },
            &no_exec
        ),
        None
    );
}

/// `devin list` runs in the role's cwd; the oldest entry active at
/// or after the role's start wins — a session a person opened later
/// in the same folder cannot.
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
            {"id": "dev-person", "last_activity_at": 140},
            {"id": "dev-taken", "last_activity_at": 150},
            {"id": "dev-new", "last_activity_at": 120}
        ]"#
        .to_string())
    };
    let role_dir = Path::new("/rd");
    let taken = BTreeSet::from(["dev-taken".to_string()]);
    // Role started at 100s: dev-old is pre-start, dev-taken is
    // claimed, dev-person came later — dev-new is the role's own.
    assert_eq!(
        find_session(
            "devin",
            &[],
            CWD,
            100_000,
            &taken,
            &Look {
                home: Path::new("/h"),
                role_dir
            },
            &exec
        )
        .as_deref(),
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
    let role_dir = Path::new("/rd");
    let failing = |_: &ExecCall| Err("no devin".to_string());
    assert_eq!(
        find_session(
            "devin",
            &[],
            CWD,
            1,
            &BTreeSet::new(),
            &Look {
                home: Path::new("/h"),
                role_dir
            },
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
            &Look {
                home: Path::new("/h"),
                role_dir
            },
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
    let role_dir = Path::new("/rd");
    let taken = BTreeSet::new();
    assert_eq!(
        find_session(
            "claude",
            &argv,
            CWD,
            0,
            &taken,
            &Look {
                home: Path::new("/h"),
                role_dir
            },
            &no_exec
        )
        .as_deref(),
        Some("uuid-1")
    );
    let taken = BTreeSet::from(["uuid-1".to_string()]);
    assert_eq!(
        find_session(
            "claude",
            &argv,
            CWD,
            0,
            &taken,
            &Look {
                home: Path::new("/h"),
                role_dir
            },
            &no_exec
        ),
        None
    );
    assert_eq!(
        find_session(
            "fake",
            &argv,
            CWD,
            0,
            &BTreeSet::new(),
            &Look {
                home: Path::new("/h"),
                role_dir
            },
            &no_exec
        ),
        None
    );
}

/// `real_exec` is the one place a command really spawns: output
/// bigger than a pipe holds must still come back whole — a child
/// that fills its stdout would otherwise deadlock against a reader
/// that only starts after exit.
#[cfg(unix)]
#[test]
fn real_exec_drains_a_full_pipe() {
    use std::time::Duration;
    let dir = tmp("exec-pipe");
    let big = dir.join("big.txt");
    fs::write(&big, "x".repeat(200_000)).unwrap();
    let call = ExecCall {
        argv: vec!["cat".into(), big.to_string_lossy().into_owned()],
        cwd: ".".into(),
        timeout: Duration::from_secs(10),
    };
    let out = real_exec(&call).unwrap();
    assert_eq!(out.len(), 200_000, "the whole file came back");
}

/// A grandchild that keeps the pipe open must not hang the join:
/// the child's own exit is the end, the reader gets a short grace
/// for what is already written, and the call returns it — a runner
/// that backgrounds a service would otherwise stop the stage loop.
#[cfg(unix)]
#[test]
fn real_exec_returns_despite_a_pipe_holding_grandchild() {
    use std::time::{Duration, Instant};
    let call = ExecCall {
        // `sleep 30` inherits stdout and holds it long past `echo`.
        argv: vec!["sh".into(), "-c".into(), "sleep 30 & echo hi".into()],
        cwd: ".".into(),
        timeout: Duration::from_secs(10),
    };
    let started = Instant::now();
    let out = real_exec(&call).unwrap();
    assert_eq!(out.trim(), "hi");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?} — the grandchild's pipe held the join",
        started.elapsed()
    );
}

/// The timeout path never joins either: a killed child whose
/// grandchild keeps the pipe returns its error at once.
#[cfg(unix)]
#[test]
fn real_exec_timeout_returns_without_the_reader() {
    use std::time::{Duration, Instant};
    let call = ExecCall {
        argv: vec!["sh".into(), "-c".into(), "sleep 30 & sleep 30".into()],
        cwd: ".".into(),
        timeout: Duration::from_millis(200),
    };
    let started = Instant::now();
    let err = real_exec(&call).unwrap_err();
    assert!(err.contains("timed out"), "{err}");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?} — the kill path waited on the reader",
        started.elapsed()
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

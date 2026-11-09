//! Tests for `discover`: fake rollout trees and injected exec output —
//! nothing here spawns a real CLI.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::discover::{ExecCall, find_session, runner_version};

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

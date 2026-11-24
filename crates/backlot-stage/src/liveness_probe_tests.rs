//! Unit tests for OS `/proc` fallback, macOS `ps` parsing, real git worktree
//! probes, and restart handling in D4 liveness tracking.

use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use backlot_model::state::RoleLastEvent;

use crate::liveness::{Evidence, ObserveInput};
use crate::liveness_diff::{WorkAreaSnap, real_work_area};
use crate::liveness_probe::{parse_macos_ps, read_proc_tree_from};
use crate::signals::Signal;
use crate::test_support::TestDir;
use crate::workareas::GitOut;

fn tmp() -> TestDir {
    TestDir::new("backlot-liveness-probe-test")
}

/// Real git work-area probe reads worktree `HEAD`, resolved branch ref,
/// `.git/index`, and runs throttled `git status --porcelain` without walking trees.
#[test]
fn real_work_area_reads_worktree_gitdir_without_walking_artifacts() {
    let dir = tmp();
    let wt = dir.join("wt");
    let gitdir = dir.join("repo/.git/worktrees/t1");
    let commondir = dir.join("repo/.git");
    fs::create_dir_all(commondir.join("refs/heads/backlot/acme")).unwrap();
    fs::create_dir_all(&gitdir).unwrap();
    fs::create_dir_all(wt.join("target/huge")).unwrap();
    fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
    fs::write(gitdir.join("commondir"), "../..\n").unwrap();
    fs::write(gitdir.join("HEAD"), "ref: refs/heads/backlot/acme/t1\n").unwrap();
    fs::write(commondir.join("refs/heads/backlot/acme/t1"), "aaa111\n").unwrap();
    fs::write(gitdir.join("index"), "idx1").unwrap();

    let calls = Arc::new(AtomicUsize::new(0));
    let calls_r = Arc::clone(&calls);
    let mut git = move |argv: &[&str], _: &str| -> GitOut {
        calls_r.fetch_add(1, Ordering::Relaxed);
        assert!(argv.ends_with(&["status", "--porcelain"]));
        GitOut {
            code: 0,
            stdout: " M src/lib.rs\n".into(),
            stderr: String::new(),
        }
    };
    let s1 = real_work_area(std::slice::from_ref(&wt), false, &mut git);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert!(s1.head_sig.contains("aaa111"), "{}", s1.head_sig);
    assert!(s1.index_mtime_ns.is_some());
    assert!(s1.status_hash.is_none());

    fs::write(commondir.join("refs/heads/backlot/acme/t1"), "bbb222\n").unwrap();
    let s2 = real_work_area(std::slice::from_ref(&wt), true, &mut git);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_ne!(s1.head_sig, s2.head_sig);
    assert!(s2.status_hash.is_some());
}

/// Linux `/proc` reader fallback (without `children` files) & macOS `ps` parser.
#[test]
fn proc_fallback_without_children_files_and_macos_ps_parser() {
    let dir = tmp();
    for (pid, ppid, comm, cmd, cpu, cutime, wchan) in [
        (10u32, 1u32, "my (agent)", "agent\0--run", 12, 5, "0"),
        (
            20,
            10,
            "cargo",
            "cargo\0test\0--lib",
            80,
            0,
            "locks_lock_inode_wait",
        ),
        (99, 1, "other", "other", 10, 0, "0"),
    ] {
        let p = dir.join(pid.to_string());
        fs::create_dir_all(&p).unwrap();
        let stat = format!("{pid} ({comm}) S {ppid} 1 1 0 -1 0 0 0 0 0 {cpu} 2 {cutime} 1 20");
        fs::write(p.join("stat"), stat).unwrap();
        fs::write(p.join("cmdline"), cmd).unwrap();
        fs::write(p.join("io"), "rchar: 120\nwchar: 80\n").unwrap();
        fs::write(p.join("wchan"), wchan).unwrap();
    }
    let tree = read_proc_tree_from(&dir, 10);
    assert_eq!(tree.len(), 2);
    assert_eq!(tree[0].pid, 10);
    assert_eq!(tree[0].depth, 0);
    assert_eq!(tree[0].cpu_jiffies, 14);
    assert_eq!(tree[0].child_cpu_jiffies, 6);
    assert_eq!(tree[0].io_bytes, 200);
    assert_eq!(tree[1].pid, 20);
    assert_eq!(tree[1].depth, 1);
    assert_eq!(tree[1].argv, "cargo test --lib");
    assert_eq!(tree[1].wchan, "locks_lock_inode_wait");

    let ps_out = "  100     1   0:02.50 agy --studio s\n  101   100   1:05.00 cargo test\n  999     1   0:10.00 unrelated\n";
    let mac = parse_macos_ps(100, ps_out);
    assert_eq!(mac.len(), 2);
    assert_eq!(mac[0].cpu_jiffies, 250);
    assert_eq!(mac[1].depth, 1);
    assert_eq!(mac[1].argv, "cargo test");
    assert_eq!(mac[1].cpu_jiffies, 6500);
}

/// Restarted stage (`stall_raised_ms == None`): preserves open stall when
/// silent, clears immediately on newer ledger or hook event.
#[test]
fn restart_keeps_stall_open_until_newer_evidence() {
    let sig = Signal {
        ts_ms: 500,
        event: "UserPromptSubmit".into(),
        notification_type: None,
        error: None,
        tool_name: None,
        tool_input: None,
    };
    let mut git = |_: &[&str], _: &str| GitOut {
        code: 0,
        stdout: String::new(),
        stderr: String::new(),
    };
    let old_ledger = RoleLastEvent {
        seq: 5,
        ts_ms: 500,
        kind: "role.started".into(),
    };
    let input = ObserveInput {
        now_ms: 2_000,
        runner: "claude",
        eligible: true,
        raw_screen: "working",
        raw_same_since_ms: 2_000,
        signal: Some(&sig),
        terminal_working: false,
        activity_revision: None,
        local_machine: false,
        pid: None,
        work_roots: &[],
        ledger: Some(&old_ledger),
    };
    let mut ev = Evidence::new(
        &input,
        &|_| Vec::new(),
        &|_, _, _| WorkAreaSnap::default(),
        &mut git,
    );
    assert!(ev.clear_reason(1_000, 2_000).is_none());

    let new_ledger = RoleLastEvent {
        seq: 9,
        ts_ms: 2_500,
        kind: "task.delivered".into(),
    };
    let input2 = ObserveInput {
        now_ms: 2_500,
        ledger: Some(&new_ledger),
        ..input
    };
    ev.observe(
        &input2,
        &|_| Vec::new(),
        &|_, _, _| WorkAreaSnap::default(),
        &mut git,
    );
    assert_eq!(ev.clear_reason(1_000, 2_500), Some("ledger event"));
}

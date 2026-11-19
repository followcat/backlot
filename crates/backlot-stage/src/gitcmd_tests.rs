//! The bounded git runner: fast commands keep their exact output and
//! exit code, a hung one gives up at the bound with a clear line, and
//! on Unix the whole process group dies with it.

use std::time::{Duration, Instant};

use crate::gitcmd::{real_git, real_git_with_timeout, run_bounded};
use crate::test_support::TestDir;

fn have_git() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_ok()
}

fn tmp() -> (TestDir, String) {
    let dir = TestDir::new("backlot-gitcmd-test");
    let cwd = dir.to_string_lossy().into_owned();
    (dir, cwd)
}

/// A fast git command returns its exact stdout, stderr and exit code.
#[test]
fn a_fast_git_returns_its_output_and_code() {
    if !have_git() {
        return;
    }
    let (_dir, cwd) = tmp();
    let version = real_git(&["--version"], &cwd);
    assert_eq!(version.code, 0, "{version:?}");
    assert!(version.stdout.starts_with("git version"), "{version:?}");
    assert!(version.stderr.is_empty(), "{version:?}");

    // A probe outside a repository is the ordinary non-zero exit — not
    // a timeout, and the captured stderr is exactly git's.
    let missing = real_git(
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            "refs/heads/does-not-exist",
        ],
        &cwd,
    );
    assert_eq!(missing.code, 128, "{missing:?}");
    assert!(missing.stdout.is_empty(), "{missing:?}");
    assert_eq!(
        missing.stderr,
        "fatal: not a git repository (or any of the parent directories): .git\n"
    );

    // The explicit-bound seam is the same runner.
    let bounded = real_git_with_timeout(&["--version"], &cwd, Duration::from_secs(60));
    assert_eq!(bounded.code, 0, "{bounded:?}");
    assert_eq!(bounded.stdout, version.stdout);
}

/// A hung `git` gives up at the bound with the actionable line and no
/// exit status of its own.
#[test]
fn a_hung_git_times_out_with_a_clear_error() {
    if !have_git() {
        return;
    }
    let (_dir, cwd) = tmp();
    // A shell alias that never returns — the exact failure a network
    // file system or credential helper can produce.
    let started = Instant::now();
    let out = real_git_with_timeout(
        &["-c", "alias.hang=!sleep 30", "hang"],
        &cwd,
        Duration::from_millis(200),
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the timeout took {:?}",
        started.elapsed()
    );
    assert_eq!(out.code, -1, "{out:?}");
    assert!(
        out.stderr.contains("git timed out after 0.2s"),
        "stderr misses the timeout line: {:?}",
        out.stderr
    );
    assert!(
        out.stderr.contains("alias.hang=!sleep 30 hang"),
        "stderr misses the argv: {:?}",
        out.stderr
    );
}

/// The timeout's SIGTERM/SIGKILL reaches the whole process group, so a
/// grandchild the command started does not survive the return.
#[cfg(unix)]
#[test]
fn a_timed_out_command_leaves_no_surviving_grandchild() {
    let (dir, cwd) = tmp();
    let pid_file = dir.join("child.pid");
    let script = format!(
        "sleep 30 & echo $! > {}; wait",
        shell_quote(&pid_file.to_string_lossy())
    );
    let started = Instant::now();
    let out = run_bounded("sh", &["-c", &script], &cwd, Duration::from_millis(200));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the timeout took {:?}",
        started.elapsed()
    );
    assert_eq!(out.code, -1, "{out:?}");
    assert!(
        out.stderr.contains("sh timed out after 0.2s"),
        "{:?}",
        out.stderr
    );
    let pid: i32 = std::fs::read_to_string(&pid_file)
        .expect("the grandchild wrote its pid")
        .trim()
        .parse()
        .expect("a pid");
    assert!(
        wait_gone(pid, Duration::from_secs(2)),
        "grandchild {pid} survived the process-group kill"
    );
}

/// Shell-quote one path for the test script.
#[cfg(unix)]
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Poll until `pid` leaves or is a zombie, bounded.
#[cfg(unix)]
fn wait_gone(pid: i32, bounded: Duration) -> bool {
    let deadline = Instant::now() + bounded;
    loop {
        if process_gone(pid) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// `true` when no live process carries `pid`. A zombie has already
/// stopped running; on Linux `/proc` names it so we can tell.
#[cfg(unix)]
fn process_gone(pid: i32) -> bool {
    // SAFETY: signal 0 only probes for existence.
    if unsafe { libc::kill(pid, 0) } != 0 {
        return true;
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            && stat.split_whitespace().nth(2) == Some("Z")
        {
            return true;
        }
        false
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// The probe path and the default runner share the same argv handling:
/// `cwd` is honoured.
#[test]
fn the_cwd_is_honoured() {
    if !have_git() {
        return;
    }
    let (dir, cwd) = tmp();
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let out = real_git(&["init", "-b", "main"], &repo.to_string_lossy());
    assert_eq!(out.code, 0, "{out:?}");
    let inside = real_git(
        &["rev-parse", "--is-inside-work-tree"],
        &repo.to_string_lossy(),
    );
    assert_eq!(inside.stdout.trim(), "true", "{inside:?}");
    // The empty temp dir is not a repo, so the same probe reads false.
    let outside = real_git(&["rev-parse", "--is-inside-work-tree"], &cwd);
    assert_ne!(outside.stdout.trim(), "true", "{outside:?}");
}

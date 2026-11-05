//! Integration tests for the compiled `hc-tui` binary.
//!
//! Every run clears the inherited environment, then gets an owned scratch
//! HOME/XDG tree plus an explicit `HC_SOCKET`/`HC_SESSION` endpoint that
//! points inside it — never at a real hc session. `PATH` is rebuilt from a
//! scratch `bin/` (first) and the system dirs, so a test can also place a
//! fake `ssh`/`hc` ahead of real tools.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SCRATCH_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Temporary HOME/XDG/bin/socket directories for one test run.
struct ScratchEnv {
    root: PathBuf,
}

impl ScratchEnv {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let unique = format!(
            "hctui-test-{}-{}-{nanos}",
            std::process::id(),
            SCRATCH_COUNTER.fetch_add(1, Ordering::Relaxed),
        );
        let root = env::temp_dir().join(unique);
        for dir in [
            "home", "state", "config", "data", "runtime", "socket", "bin",
        ] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        Self { root }
    }

    fn missing_socket(&self) -> PathBuf {
        self.root.join("socket").join("missing.sock")
    }
}

impl Drop for ScratchEnv {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// A `Command` for the compiled hc-tui binary with nothing inherited and
/// only owned scratch context: HOME, the XDG dirs, `HC_SOCKET`/`HC_SESSION`
/// aimed inside the scratch tree, and PATH led by the empty scratch `bin/`.
fn command(scratch: &ScratchEnv, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_hc-tui"));
    cmd.args(args)
        .env_clear()
        // PATH is literal: scratch bin/ (a --remote test must install a
        // fake ssh there first — OpenSSH can still read passwd-home
        // config despite the scratch HOME) plus the system dirs. Nothing
        // inherited.
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", scratch.root.join("bin").display()),
        )
        .env("HOME", scratch.root.join("home"))
        .env("HC_SOCKET", scratch.missing_socket())
        .env("HC_SESSION", "hctui-test-scratch")
        .env("XDG_STATE_HOME", scratch.root.join("state"))
        .env("XDG_CONFIG_HOME", scratch.root.join("config"))
        .env("XDG_DATA_HOME", scratch.root.join("data"))
        .env("XDG_RUNTIME_DIR", scratch.root.join("runtime"))
        .env("TERM", "xterm-256color")
        .current_dir(&scratch.root);
    cmd
}

/// Runs hc-tui to completion under the isolated scratch environment.
fn run_hc_tui(scratch: &ScratchEnv, args: &[&str]) -> Output {
    command(scratch, args).output().unwrap()
}

/// Asserts an isolated child env leaks nothing real: every endpoint/config
/// variable points inside `scratch`.
fn assert_env_isolated(scratch: &ScratchEnv, cmd: &Command) {
    for var in [
        "HOME",
        "HC_SOCKET",
        "XDG_STATE_HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_RUNTIME_DIR",
    ] {
        let val = cmd
            .get_envs()
            .find(|(k, _)| k == &OsStr::new(var))
            .and_then(|(_, v)| v)
            .map(Path::new);
        assert!(
            val.is_some_and(|p| p.starts_with(&scratch.root)),
            "{var} escaped the scratch root: {val:?}"
        );
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn version_prints_name_and_package_version() {
    let scratch = ScratchEnv::new();
    let output = run_hc_tui(&scratch, &["--version"]);
    assert!(output.status.success());
    assert_eq!(
        stdout(&output).trim(),
        format!("hc-tui {}", env!("CARGO_PKG_VERSION")),
    );
}

#[test]
fn help_describes_options_and_keys_without_connecting() {
    let scratch = ScratchEnv::new();
    let output = run_hc_tui(&scratch, &["--help"]);
    assert!(output.status.success());
    let stdout = stdout(&output);
    assert!(stdout.contains("--help"));
    assert!(stdout.contains("--version"));
    assert!(stdout.contains("--session"));
    assert!(stdout.contains("--remote"));
    assert!(stdout.contains("Ctrl+B"));
    // Help answers before anything connects — the missing socket must
    // still be missing (no attempt wrote anything there).
    assert!(!scratch.missing_socket().exists());
}

#[test]
fn a_non_tty_run_reports_terminal_setup_and_stops() {
    let scratch = ScratchEnv::new();
    assert!(!scratch.missing_socket().exists());
    assert_env_isolated(&scratch, &command(&scratch, &[]));
    // With no controlling TTY (Command::output is piped), the run must
    // fail fast at terminal setup — bounded, not hung reconnecting.
    let output = run_hc_tui(&scratch, &[]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("terminal"), "stderr: {stderr}");
    assert!(!scratch.missing_socket().exists());
}

#[test]
fn unknown_argument_is_rejected() {
    let scratch = ScratchEnv::new();
    let output = run_hc_tui(&scratch, &["--bogus"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("--bogus"));
    assert!(stderr.contains("--help"));
}

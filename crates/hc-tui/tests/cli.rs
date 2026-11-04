//! Integration tests for the compiled `hc-tui` binary.
//!
//! Every run points `HC_SOCKET` at a path that does not exist and sets
//! temporary XDG directories, so a test can never inspect or reach a real hc
//! session.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SCRATCH_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Temporary XDG directories and socket directory for one test run.
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
        for dir in ["state", "config", "runtime", "socket"] {
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

/// Runs the compiled hc-tui binary with every hc endpoint variable aimed at
/// the scratch environment, never at a real session.
fn run_hc_tui(scratch: &ScratchEnv, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hc-tui"))
        .args(args)
        .env("HC_SOCKET", scratch.missing_socket())
        .env("HC_SESSION", "hctui-test-scratch")
        .env("XDG_STATE_HOME", scratch.root.join("state"))
        .env("XDG_CONFIG_HOME", scratch.root.join("config"))
        .env("XDG_RUNTIME_DIR", scratch.root.join("runtime"))
        .output()
        .unwrap()
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
fn help_describes_bootstrap_status_and_options() {
    let scratch = ScratchEnv::new();
    let output = run_hc_tui(&scratch, &["--help"]);
    assert!(output.status.success());
    let stdout = stdout(&output);
    assert!(stdout.contains("--help"));
    assert!(stdout.contains("--version"));
    assert!(stdout.contains("not implemented"));
    assert!(stdout.contains("does not connect"));
}

#[test]
fn default_run_reports_bootstrap_without_connecting() {
    let scratch = ScratchEnv::new();
    assert!(!scratch.missing_socket().exists());
    let output = run_hc_tui(&scratch, &[]);
    assert!(output.status.success());
    let stdout = stdout(&output);
    assert!(stdout.contains("bootstrap"));
    assert!(stdout.contains(&hc_proto::PROTOCOL_GENERATION.to_string()));
    assert!(stdout.contains("No connection"));
    // A connection attempt would have created nothing and failed; the missing
    // socket must still be missing.
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

//! Shared harness for the binary-level studio tests: a scratch
//! HOME/BACKLOT_STATE_DIR and wrappers that spawn the compiled
//! `backlot` binary. Nothing touches a real hc session — the same
//! isolation as `tests/cli.rs` applies.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SCRATCH_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Parent of the scratch tree. macOS's per-user `$TMPDIR` is so long
/// that the socket path for `--session` would exceed `sun_path` and
/// tests would fail at endpoint resolution instead of what they assert.
#[cfg(target_os = "macos")]
fn scratch_parent() -> PathBuf {
    PathBuf::from("/tmp")
}

#[cfg(not(target_os = "macos"))]
fn scratch_parent() -> PathBuf {
    std::env::temp_dir()
}

/// Scratch HOME/XDG/BACKLOT_STATE_DIR for one test.
pub struct Scratch {
    pub root: PathBuf,
    pub state: PathBuf,
}

impl Scratch {
    pub fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let unique = format!(
            "backlot-studio-test-{}-{}-{nanos}",
            std::process::id(),
            SCRATCH_COUNTER.fetch_add(1, Ordering::Relaxed),
        );
        let root = scratch_parent().join(unique);
        let state = root.join("backlot-state");
        for dir in ["home", "files"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::create_dir_all(&state).unwrap();
        Self { root, state }
    }

    pub fn fixture(&self, name: &str, content: &str) -> String {
        let p = self.root.join("files").join(name);
        fs::write(&p, content).unwrap();
        p.to_string_lossy().into_owned()
    }

    pub fn events(&self) -> PathBuf {
        self.state.join("studios/acme-shop/events.jsonl")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub fn command(scratch: &Scratch, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_backlot"));
    cmd.args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", scratch.root.join("home"))
        .env("BACKLOT_STATE_DIR", &scratch.state)
        .env("BACKLOT_STUDIO", "acme-shop")
        .env("HC_SOCKET", scratch.root.join("missing.sock"))
        .env("TERM", "xterm-256color")
        .current_dir(&scratch.root);
    cmd
}

pub fn run(scratch: &Scratch, args: &[&str]) -> Output {
    command(scratch, args).output().unwrap()
}

pub fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

pub fn stderr(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}

pub fn ok(scratch: &Scratch, args: &[&str]) -> String {
    let o = run(scratch, args);
    assert!(
        o.status.success(),
        "{args:?} exited {:?}: {}",
        o.status.code(),
        stderr(&o)
    );
    stdout(&o)
}

pub const CAST_TOML: &str = r#"
mode = "standard"
rework_cap = 2

[producer]
runner = "runner-p"
model = "model-p"
effort = "high"
permission_profile = "standard"

[director]
runner = "runner-d"
model = "model-d"
effort = "high"
permission_profile = "standard"

[writer]
runner = "runner-w"
model = "model-w"
effort = "medium"
permission_profile = "standard"

[[candidates]]
name = "actor-a"
runner = "runner-a"
model = "model-a"
effort = "high"
permission_profile = "worktree"

[[candidates]]
name = "actor-b"
runner = "runner-b"
model = "model-b"
effort = "low"
permission_profile = "worktree"
"#;

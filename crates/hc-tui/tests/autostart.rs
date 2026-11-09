//! The compiled `hc-tui` hosted in a PTY with a fake `hc` on `PATH`
//! that records its invocations: the one-shot local core start fires
//! exactly when it should and reports on screen when it can't.

#![cfg(unix)]

#[path = "common/pty.rs"]
mod pty;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use hc_vt::Vt;
use pty::{POLL, Pty, screen};

const COLS: u16 = 120;
const ROWS: u16 = 34;
/// Finite bound on any single step.
const DEADLINE: Duration = Duration::from_secs(10);
/// How long "nothing must happen" tests settle before asserting the
/// fake was never invoked — far past the start task's dispatch.
const SETTLE: Duration = Duration::from_secs(2);

static SCRATCH_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Temporary HOME/XDG/bin directories for one PTY run.
struct ScratchEnv {
    root: PathBuf,
}

impl ScratchEnv {
    fn new() -> Self {
        // Short root: the resolved `herdr-core/<session>.holder.sock`
        // under it must fit `sun_path`.
        let unique = format!(
            "hctui-as-{}-{}",
            std::process::id(),
            SCRATCH_COUNTER.fetch_add(1, Ordering::Relaxed),
        );
        let root = scratch_parent().join(unique);
        for dir in ["home", "state", "config", "data", "runtime", "bin", "work"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        Self { root }
    }

    /// The socket endpoint resolution picks without `HC_SOCKET`:
    /// `$XDG_RUNTIME_DIR/herdr-core/<session>.sock`.
    fn resolved_socket(&self, session: &str) -> PathBuf {
        self.root
            .join("runtime")
            .join("herdr-core")
            .join(format!("{session}.sock"))
    }
}

impl Drop for ScratchEnv {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Parent of the scratch tree, kept short for `sun_path`.
#[cfg(target_os = "macos")]
fn scratch_parent() -> PathBuf {
    PathBuf::from("/tmp")
}

#[cfg(not(target_os = "macos"))]
fn scratch_parent() -> PathBuf {
    std::env::temp_dir()
}

/// A fake `hc` in the scratch `bin/`: appends its argv plus the
/// `HC_SESSION` it saw to `record`, then exits per `mode` — `ok`
/// succeeds, `fail` prints two stderr lines and exits 1.
fn fake_hc(scratch: &ScratchEnv, mode: &str) -> PathBuf {
    let record = scratch.root.join("hc.record");
    let tail = match mode {
        "fail" => "echo 'fake: first failure line' >&2\necho 'fake: second' >&2\nexit 1",
        _ => "exit 0",
    };
    let script = scratch.root.join("bin").join("hc");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf 'hc %s session=%s\\n' \"$*\" \"${{HC_SESSION-}}\" \
             >> \"{}\"\n{tail}\n",
            record.display(),
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    record
}

/// A fake `ssh` in the scratch `bin/` that records and fails at once —
/// a `--remote` run must never reach a real ssh.
fn fake_ssh(scratch: &ScratchEnv) {
    let script = scratch.root.join("bin").join("ssh");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf 'ssh %s\\n' \"$*\" >> \"{}\"\nexit 1\n",
            scratch.root.join("ssh.record").display(),
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
}

/// The environment `hc-tui` sees: nothing inherited, scratch `bin/`
/// first on `PATH`, XDG roots inside the scratch tree, `HC_SESSION`
/// set — `HC_SOCKET` only when the test asks for it.
fn tui_env(scratch: &ScratchEnv, session: &str, extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut env = vec![
        (
            "PATH".into(),
            format!("{}:/usr/bin:/bin", scratch.root.join("bin").display()),
        ),
        ("TERM".into(), "xterm-256color".into()),
        (
            "HOME".into(),
            scratch.root.join("home").display().to_string(),
        ),
        (
            "XDG_STATE_HOME".into(),
            scratch.root.join("state").display().to_string(),
        ),
        (
            "XDG_CONFIG_HOME".into(),
            scratch.root.join("config").display().to_string(),
        ),
        (
            "XDG_DATA_HOME".into(),
            scratch.root.join("data").display().to_string(),
        ),
        (
            "XDG_RUNTIME_DIR".into(),
            scratch.root.join("runtime").display().to_string(),
        ),
        ("HC_SESSION".into(), session.into()),
    ];
    env.extend(extra.iter().map(|(k, v)| ((*k).into(), (*v).into())));
    env
}

fn tui_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_backlot"))
}

fn spawn_tui(scratch: &ScratchEnv, args: &[&str], session: &str, extra: &[(&str, &str)]) -> Pty {
    Pty::spawn(
        &tui_bin(),
        args,
        &tui_env(scratch, session, extra),
        &scratch.root.join("work"),
        COLS,
        ROWS,
    )
}

/// Pump for `dur` — lets the run settle and any wrongly started fake
/// record itself.
fn settle(pty: &Pty, vt: &mut Vt, dur: Duration) {
    let deadline = Instant::now() + dur;
    while Instant::now() < deadline {
        pty.pump(vt);
        std::thread::sleep(POLL);
    }
}

/// Pump until `pred` holds on the screen — bounded, always.
fn until_screen(pty: &Pty, vt: &mut Vt, pred: impl Fn(&str) -> bool, what: &str) {
    let deadline = Instant::now() + DEADLINE;
    loop {
        pty.pump(vt);
        let text = screen(vt);
        if pred(&text) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; screen was:\n{text}"
        );
        std::thread::sleep(POLL);
    }
}

/// `Ctrl+B q` detach, then a bounded wait for the child's exit. Keeps
/// draining the master: a child blocked writing to a full PTY never
/// reaches its exit.
fn detach_and_wait(pty: &mut Pty, vt: &mut Vt) -> std::process::ExitStatus {
    pty.type_keys(b"\x02q");
    let deadline = Instant::now() + DEADLINE;
    loop {
        pty.pump(vt);
        match pty.child.try_wait().expect("try_wait") {
            Some(status) => return status,
            None => {
                assert!(
                    Instant::now() < deadline,
                    "hc-tui did not exit after detach"
                );
                std::thread::sleep(POLL);
            }
        }
    }
}

/// Wait for `record` to appear, bounded — the fake writes it as soon
/// as it is invoked.
fn wait_record(pty: &Pty, vt: &mut Vt, record: &Path) -> String {
    let deadline = Instant::now() + DEADLINE;
    loop {
        pty.pump(vt);
        if record.exists() {
            return fs::read_to_string(record).unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "the fake hc was never invoked; screen:\n{}",
            screen(vt)
        );
        std::thread::sleep(POLL);
    }
}

/// A dead local socket runs `hc server start` once — with the
/// `--session`-resolved session, not `HC_SESSION`'s — and the second
/// half of the run never re-runs it.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "Ctrl+B q doesn't end hc-tui in a macOS PTY yet"
)]
fn a_dead_local_socket_starts_the_core_once() {
    let scratch = ScratchEnv::new();
    let record = fake_hc(&scratch, "ok");
    // env-session would be picked without the override; arg-session
    // must win all the way into the child's environment.
    let mut pty = spawn_tui(&scratch, &["--session", "arg-session"], "env-session", &[]);
    let mut vt = Vt::new(COLS, ROWS, 4096);
    let recorded = wait_record(&pty, &mut vt, &record);
    assert_eq!(recorded.trim(), "hc server start session=arg-session");
    settle(&pty, &mut vt, SETTLE);
    assert_eq!(
        fs::read_to_string(&record).unwrap().trim(),
        "hc server start session=arg-session",
        "the fake ran more than once"
    );
    assert!(detach_and_wait(&mut pty, &mut vt).success());
}

/// `--remote` resolves a remote endpoint: the local core start never
/// applies, whatever the socket looks like.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "Ctrl+B q doesn't end hc-tui in a macOS PTY yet"
)]
fn a_remote_run_never_starts_a_local_core() {
    let scratch = ScratchEnv::new();
    let record = fake_hc(&scratch, "ok");
    fake_ssh(&scratch);
    let mut pty = spawn_tui(&scratch, &["--remote", "host-a"], "hctui-test-scratch", &[]);
    let mut vt = Vt::new(COLS, ROWS, 4096);
    settle(&pty, &mut vt, SETTLE);
    assert!(detach_and_wait(&mut pty, &mut vt).success());
    assert!(!record.exists(), "remote ran the local fake: {record:?}");
}

/// `HC_SOCKET` names a specific core someone else manages — set, it
/// vetoes the start even while the socket answers nothing.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "Ctrl+B q doesn't end hc-tui in a macOS PTY yet"
)]
fn a_set_hc_socket_never_starts_a_core() {
    let scratch = ScratchEnv::new();
    let record = fake_hc(&scratch, "ok");
    let socket = scratch.root.join("elsewhere.sock");
    let mut pty = spawn_tui(
        &scratch,
        &[],
        "hctui-test-scratch",
        &[("HC_SOCKET", socket.to_str().unwrap())],
    );
    let mut vt = Vt::new(COLS, ROWS, 4096);
    settle(&pty, &mut vt, SETTLE);
    assert!(detach_and_wait(&mut pty, &mut vt).success());
    assert!(!record.exists(), "HC_SOCKET set yet the fake ran");
}

/// Something listening on the resolved socket means a core answers —
/// nothing is started, even though the listener never speaks.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "Ctrl+B q doesn't end hc-tui in a macOS PTY yet"
)]
fn an_answering_socket_never_starts_a_core() {
    let scratch = ScratchEnv::new();
    let session = "hctui-test-scratch";
    let record = fake_hc(&scratch, "ok");
    let socket = scratch.resolved_socket(session);
    fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let _listener = UnixListener::bind(&socket).unwrap();
    let mut pty = spawn_tui(&scratch, &[], session, &[]);
    let mut vt = Vt::new(COLS, ROWS, 4096);
    settle(&pty, &mut vt, SETTLE);
    assert!(detach_and_wait(&mut pty, &mut vt).success());
    assert!(!record.exists(), "a live socket still started the fake");
}

/// No `hc` on `PATH`: the run keeps today's behavior (UI up, retrying)
/// plus a notice naming the official installer.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "Ctrl+B q doesn't end hc-tui in a macOS PTY yet"
)]
fn a_missing_hc_shows_the_install_notice() {
    let scratch = ScratchEnv::new();
    let mut pty = spawn_tui(&scratch, &[], "hctui-test-scratch", &[]);
    let mut vt = Vt::new(COLS, ROWS, 4096);
    until_screen(
        &pty,
        &mut vt,
        |text| text.contains("releases.herdr.dev"),
        "the missing-hc notice",
    );
    assert!(detach_and_wait(&mut pty, &mut vt).success());
}

/// `hc server start` failing shows its first error line.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "Ctrl+B q doesn't end hc-tui in a macOS PTY yet"
)]
fn a_failed_start_shows_its_first_error_line() {
    let scratch = ScratchEnv::new();
    let record = fake_hc(&scratch, "fail");
    let mut pty = spawn_tui(&scratch, &[], "hctui-test-scratch", &[]);
    let mut vt = Vt::new(COLS, ROWS, 4096);
    let recorded = wait_record(&pty, &mut vt, &record);
    assert_eq!(
        recorded.trim(),
        "hc server start session=hctui-test-scratch"
    );
    until_screen(
        &pty,
        &mut vt,
        |text| text.contains("fake: first failure line"),
        "the failed-start notice",
    );
    assert!(detach_and_wait(&mut pty, &mut vt).success());
}

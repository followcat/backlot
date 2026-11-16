//! Opt-in real-core Waiting coverage: the actual `hc-tui` binary in a
//! PTY against the core at `HC_TUI_TEST_CORE`, in a `ScratchCore`
//! session. A fake agent reports over OSC 7501 and the probe layers —
//! process tree and the Claude sessions file — drive the `◌` overlay.
//!
//! Ignored by default: `cargo test` never touches a real core. Run
//! explicitly with
//!
//! ```sh
//! HC_TUI_TEST_CORE=/path/to/pinned/herdr-core \
//!     cargo test -p hc-tui --test waiting_e2e -- --ignored --test-threads=1
//! ```
//!
//! Set `HC_TUI_WAITING_CAPTURE` to a directory to save the Waiting and
//! post-clear screens as text, one file per case.

#![cfg(unix)]

#[path = "../../hc-proto/tests/common/scratch.rs"]
mod scratch;

use scratch::ScratchCore;

use hc_proto::client::Client;
use hc_proto::endpoint::Endpoint;
use hc_proto::terminal::{ListParams, RunParams};
use hc_vt::Vt;

use std::collections::BTreeMap;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Geometry big enough for the sidebar plus a usable pane.
const COLS: u16 = 120;
const ROWS: u16 = 34;
/// Finite bound on any single real-core step.
const DEADLINE: Duration = Duration::from_secs(30);
/// Read pump cadence while waiting on a screen predicate.
const POLL: Duration = Duration::from_millis(40);
/// The agent must outlive the probe's ~20 s start window before its
/// tool shells count — keep a real margin for slow hosts.
const START_WINDOW: Duration = Duration::from_secs(24);
/// The Waiting overlay's icon (docs/sidebar.md).
const WAITING_ICON: char = '◌';

/// The `hc-tui` binary under test — cargo exports it for this target.
fn tui_bin() -> &'static str {
    env!("CARGO_BIN_EXE_hc-tui")
}

/// The environment `hc-tui` sees: the scratch fixture's own, nothing
/// inherited — a real user session can never leak in.
fn tui_env(scratch: &ScratchCore) -> Vec<(String, String)> {
    let root = |dir: &str| scratch.root.join(dir).to_string_lossy().into_owned();
    let Endpoint::Local(local) = &scratch.endpoint else {
        panic!("scratch endpoint is always local");
    };
    vec![
        ("PATH".into(), "/usr/bin:/bin".into()),
        ("LANG".into(), "C.UTF-8".into()),
        ("TERM".into(), "xterm-256color".into()),
        ("HOME".into(), root("home")),
        ("XDG_STATE_HOME".into(), root("state")),
        ("XDG_CONFIG_HOME".into(), root("config")),
        ("XDG_RUNTIME_DIR".into(), root("run")),
        ("HC_SESSION".into(), scratch.session.clone()),
        (
            "HC_SOCKET".into(),
            local.socket.to_string_lossy().into_owned(),
        ),
    ]
}

/// A pty pair at `cols`x`rows`: nonblocking master for the pump.
#[allow(clippy::unnecessary_mut_passed)]
fn openpty(cols: u16, rows: u16) -> (OwnedFd, OwnedFd) {
    unsafe {
        let mut ws = libc::winsize {
            ws_col: cols,
            ws_row: rows,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let (mut master, mut slave) = (-1, -1);
        assert_eq!(
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut ws,
            ),
            0,
            "openpty: {:?}",
            std::io::Error::last_os_error()
        );
        let flags = libc::fcntl(master, libc::F_GETFL);
        assert_eq!(
            libc::fcntl(master, libc::F_SETFL, flags | libc::O_NONBLOCK),
            0
        );
        (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave))
    }
}

/// `hc-tui` hosted in a PTY.
struct Tui {
    master: OwnedFd,
    child: Child,
}

fn spawn_tui(scratch: &ScratchCore) -> Tui {
    let (master, slave) = openpty(COLS, ROWS);
    let sfd = slave.as_raw_fd();
    let dup = || unsafe { Stdio::from_raw_fd(libc::dup(sfd)) };
    let mut command = Command::new(tui_bin());
    command
        .env_clear()
        .envs(tui_env(scratch))
        .current_dir(&scratch.work)
        .stdin(dup())
        .stdout(dup())
        .stderr(dup());
    unsafe {
        command.pre_exec(move || {
            if libc::login_tty(sfd) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command
        .spawn()
        .unwrap_or_else(|e| panic!("cannot spawn {}: {e}", tui_bin()));
    Tui { master, child }
}

impl Tui {
    /// Everything the child wrote since the last pump, fed to `vt`;
    /// replies the emulated terminal owes go back.
    fn pump(&self, vt: &mut Vt) {
        let mut buf = [0u8; 65536];
        loop {
            let n =
                unsafe { libc::read(self.master.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                break;
            }
            vt.feed(&buf[..n as usize]);
        }
        let replies = vt.take_replies().unwrap_or_default();
        if !replies.is_empty() {
            unsafe {
                libc::write(
                    self.master.as_raw_fd(),
                    replies.as_ptr().cast(),
                    replies.len(),
                );
            }
        }
    }

    /// Keystrokes for the TUI.
    fn type_keys(&self, keys: &[u8]) {
        let mut written = 0;
        while written < keys.len() {
            let n = unsafe {
                libc::write(
                    self.master.as_raw_fd(),
                    keys[written..].as_ptr().cast(),
                    keys.len() - written,
                )
            };
            assert!(n > 0, "pty write failed");
            written += n as usize;
        }
    }

    /// A panic path still kills the child — an orphaned TUI keeps the
    /// fixture env's inherited fds (the suite lock included) open.
    fn detach(mut self) {
        self.type_keys(b"\x02q"); // prefix+q
        let deadline = Instant::now() + DEADLINE;
        loop {
            match self.child.try_wait().expect("try_wait") {
                Some(status) => {
                    assert!(status.success(), "hc-tui exited with {status}");
                    return;
                }
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
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The whole emulated screen as text rows.
fn screen(vt: &Vt) -> String {
    let (cols, rows) = vt.size().expect("vt size");
    (0..rows)
        .map(|row| vt.screen_row_text(row.into(), cols).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Pump until `pred` holds on the screen — bounded, always.
async fn until_screen(tui: &Tui, vt: &mut Vt, pred: impl Fn(&Vt) -> bool, what: &str) {
    let deadline = Instant::now() + DEADLINE;
    loop {
        tui.pump(vt);
        if pred(vt) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; screen was:\n{}",
            screen(vt)
        );
        tokio::time::sleep(POLL).await;
    }
}

async fn connect(scratch: &ScratchCore) -> Client {
    Client::connect(&scratch.endpoint, None::<&std::ffi::OsStr>)
        .await
        .expect("connect to scratch core")
}

/// `terminal.run` a named program under the scratch work directory.
async fn run(scratch: &ScratchCore, client: &Client, name: &str, argv: Vec<String>) {
    client
        .terminal_run(RunParams {
            argv,
            name: Some(name.into()),
            cwd: Some(scratch.work.to_string_lossy().into_owned()),
            labels: BTreeMap::from([("herdr.seen".into(), "0".into())]),
            ..RunParams::default()
        })
        .await
        .unwrap_or_else(|e| panic!("terminal.run {name}: {e}"));
}

/// A shell that reports `state` once over OSC 7501, then stays
/// interactive — an agent that still answers typed input.
fn reporter(state: &str) -> Vec<String> {
    vec![
        "sh".into(),
        "-c".into(),
        format!("printf '\\033]7501;state={state}\\033\\\\'; exec sh"),
    ]
}

/// The foreground pid the core reports for one terminal — retried
/// while the fg resolution catches up with the spawn.
async fn foreground_pid(client: &Client, name: &str) -> i32 {
    let deadline = Instant::now() + DEADLINE;
    loop {
        if let Some(pid) = client
            .terminal_list(ListParams::default())
            .await
            .expect("terminal.list")
            .terminals
            .iter()
            .find(|t| t.name.as_deref() == Some(name))
            .and_then(|t| t.foreground.as_ref().map(|f| f.pid))
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "{name}: no live foreground");
        tokio::time::sleep(POLL).await;
    }
}

/// Whether the Waiting glyph is on screen anywhere.
fn shows_waiting(vt: &Vt) -> bool {
    screen(vt).contains(WAITING_ICON)
}

/// Save the screen text when `HC_TUI_WAITING_CAPTURE` names a
/// directory — evidence next to the report, never required.
fn capture(tag: &str, vt: &Vt) {
    if let Some(dir) = std::env::var_os("HC_TUI_WAITING_CAPTURE") {
        std::fs::create_dir_all(&dir).ok();
        let path = std::path::Path::new(&dir).join(format!("{tag}.txt"));
        std::fs::write(&path, screen(vt)).expect("capture write");
    }
}

/// Wait for a real agent idle long enough that its tool shells count
/// (past the probe's start window).
async fn past_start_window() {
    tokio::time::sleep(START_WINDOW).await;
}

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn waiting_process_layer_shows_and_clears() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    assert!(
        client.explain_unavailable("machine.exec").is_none(),
        "the pinned core must offer machine.exec for the process layer"
    );
    let tui = spawn_tui(&scratch);
    let mut vt = Vt::new(COLS, ROWS, 4096);

    run(&scratch, &client, "wait-ps", reporter("idle:app=devin")).await;
    // `app` replaces `name` as the row label — `display_agent` rule.
    until_screen(&tui, &mut vt, |v| screen(v).contains("devin"), "agent row").await;
    // The tool shell must start past the ~20 s agent-start window.
    past_start_window().await;

    // A background `sh -c` of the agent's interactive shell — exactly
    // the generic layer's evidence.
    tui.type_keys(b"sh -c 'sleep 60' &\r");
    until_screen(&tui, &mut vt, shows_waiting, "Waiting overlay").await;
    capture("waiting-process", &vt);

    // The background work ending returns the row to Idle.
    tui.type_keys(b"kill %1\r");
    until_screen(&tui, &mut vt, |v| !shows_waiting(v), "overlay clears").await;
    capture("waiting-process-cleared", &vt);

    tui.detach();
    drop(client);
    scratch.finish().expect("scratch teardown");
}

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn waiting_claude_sessions_file_shows_and_clears() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    for method in ["machine.exec", "file.read"] {
        assert!(
            client.explain_unavailable(method).is_none(),
            "the pinned core must offer {method} for the Claude layer"
        );
    }
    let tui = spawn_tui(&scratch);
    let mut vt = Vt::new(COLS, ROWS, 4096);

    run(
        &scratch,
        &client,
        "wait-claude",
        reporter("idle:app=claude-code"),
    )
    .await;
    until_screen(
        &tui,
        &mut vt,
        |v| screen(v).contains("claude-code"),
        "agent row",
    )
    .await;

    // The sessions file lives under the core's own scratch HOME; only
    // `pid` and `status` matter to the probe.
    let pid = foreground_pid(&client, "wait-claude").await;
    let sessions = scratch.root.join("home/.claude/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let file = sessions.join(format!("{pid}.json"));
    let write_status = |status: &str| {
        std::fs::write(&file, format!(r#"{{"pid":{pid},"status":"{status}"}}"#)).unwrap();
    };

    write_status("shell");
    until_screen(&tui, &mut vt, shows_waiting, "Waiting overlay").await;
    capture("waiting-claude", &vt);

    write_status("idle");
    until_screen(&tui, &mut vt, |v| !shows_waiting(v), "overlay clears").await;
    capture("waiting-claude-cleared", &vt);

    tui.detach();
    drop(client);
    scratch.finish().expect("scratch teardown");
}

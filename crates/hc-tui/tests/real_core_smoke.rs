//! Opt-in real-core PTY smoke: the actual `hc-tui` binary hosted in a
//! PTY against the core at `HC_TUI_TEST_CORE`, in a `ScratchCore`-style
//! isolated session. Assertions read the emulated screen (`hc-vt`), so
//! they hold on any generation-1 core — fork and official alike.
//!
//! Ignored by default: `cargo test` never touches a real core. Run
//! explicitly with
//!
//! ```sh
//! HC_TUI_TEST_CORE=$(command -v hc) \
//!     cargo test -p hc-tui --test real_core_smoke -- --ignored --test-threads=1
//! ```
//!
//! Covers: idle start, two terminals in the sidebar, attach + echo,
//! pane switching, tab and split creation, an OSC 7501 status word in
//! the sidebar, and a clean detach with restored terminal modes.

#![cfg(unix)]

#[path = "../../hc-proto/tests/common/scratch.rs"]
mod scratch;

use scratch::ScratchCore;

use hc_proto::client::Client;
use hc_proto::endpoint::Endpoint;
use hc_proto::terminal::{ListParams, ReadParams, RunParams};
use hc_vt::Vt;

use std::collections::BTreeMap;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Geometry big enough for sidebar plus a usable pane.
const COLS: u16 = 120;
const ROWS: u16 = 34;
/// Finite bound on any single real-core step.
const DEADLINE: Duration = Duration::from_secs(20);
/// Read pump cadence while waiting on a screen predicate.
const POLL: Duration = Duration::from_millis(40);

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

/// A pty pair at `cols`x`rows`: nonblocking master for the pump, slave
/// kept open so the final termios check still has a handle after the
/// child exits. `openpty` applies the size atomically on every Unix.
// Apple's openpty takes *mut winsize; the &mut is needed there and
// harmless on Linux where it coerces to *const.
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

/// `hc-tui` hosted in a PTY: own session, controlling terminal, the
/// slave's initial modes captured for the restore check.
struct Tui {
    master: OwnedFd,
    slave: OwnedFd,
    initial_modes: libc::termios,
    child: Child,
}

fn spawn_tui(scratch: &ScratchCore) -> Tui {
    let (master, slave) = openpty(COLS, ROWS);
    let mut initial_modes: libc::termios = unsafe { std::mem::zeroed() };
    unsafe {
        assert_eq!(libc::tcgetattr(slave.as_raw_fd(), &mut initial_modes), 0);
    }
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
            // New session + controlling tty, matching a terminal
            // launcher: login_tty is setsid + TIOCSCTTY + stdio dup on
            // every Unix (libc has no TIOCSCTTY name on Apple targets).
            if libc::login_tty(sfd) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command
        .spawn()
        .unwrap_or_else(|e| panic!("cannot spawn {}: {e}", tui_bin()));
    Tui {
        master,
        slave,
        initial_modes,
        child,
    }
}

impl Tui {
    /// Everything the child wrote since the last pump, fed to `vt`;
    /// replies the emulated terminal owes (queries, reports) go back.
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

    /// Keystrokes for the TUI: plain text types through; chords are the
    /// prefix (Ctrl+B = `\x02`) plus the bound key.
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

/// `terminal.run` a named program under the scratch work directory —
/// one shared cwd keeps every agent in the same workspace and tab.
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

/// A shell that reports `state` once over OSC 7501, then stays — an
/// agent (`status.is_some()`) that still answers typed input.
fn reporter(state: &str) -> Vec<String> {
    vec![
        "sh".into(),
        "-c".into(),
        format!("printf '\\033]7501;state={state}\\033\\\\'; exec sh"),
    ]
}

/// Live terminal count the core reports.
async fn terminal_count(client: &Client) -> usize {
    client
        .terminal_list(ListParams::default())
        .await
        .expect("terminal.list")
        .terminals
        .len()
}

/// `terminal.read` for one terminal — just the text.
async fn read_text(client: &Client, terminal: &str) -> String {
    client
        .terminal_read(ReadParams {
            terminal: terminal.into(),
            lines: 50,
            caller: None,
        })
        .await
        .expect("terminal.read")
        .text
}

/// Pump for `dur` so the app can process keys and repaint.
async fn settle(tui: &Tui, vt: &mut Vt, dur: Duration) {
    let deadline = Instant::now() + dur;
    while Instant::now() < deadline {
        tui.pump(vt);
        tokio::time::sleep(POLL).await;
    }
}

/// Type `keys` at whichever pane is focused until `terminal.read` on
/// `names` reports `needle`, retyping while the attach may still be
/// opening. Returns the terminal that received the input.
async fn type_until_read(
    tui: &Tui,
    vt: &mut Vt,
    client: &Client,
    names: &[&str],
    keys: &[u8],
    needle: &str,
) -> String {
    let deadline = Instant::now() + DEADLINE;
    let mut typed = false;
    loop {
        if std::mem::replace(&mut typed, true) {
            tokio::time::sleep(POLL * 10).await;
        }
        tui.type_keys(keys);
        tui.pump(vt);
        let settle_deadline = (Instant::now() + Duration::from_millis(400)).min(deadline);
        loop {
            for name in names {
                if read_text(client, name).await.contains(needle) {
                    return (*name).to_owned();
                }
            }
            if Instant::now() >= settle_deadline {
                break;
            }
            tui.pump(vt);
            tokio::time::sleep(POLL).await;
        }
        assert!(
            Instant::now() < deadline,
            "typed marker {needle:?} never reached any terminal"
        );
    }
}

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_core_smoke() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let mut tui = spawn_tui(&scratch);
    let mut vt = Vt::new(COLS, ROWS, 4096);

    // 1. No terminals: the UI still renders — chrome, not an error.
    until_screen(
        &tui,
        &mut vt,
        |v| screen(v).contains("agents"),
        "initial UI render",
    )
    .await;

    // 2. Two shells: both names reach the sidebar.
    run(&scratch, &client, "smoke-a", reporter("idle")).await;
    run(&scratch, &client, "smoke-b", reporter("idle")).await;
    until_screen(
        &tui,
        &mut vt,
        |v| {
            let s = screen(v);
            s.contains("smoke-a") && s.contains("smoke-b")
        },
        "both terminals in the sidebar",
    )
    .await;

    // 3. Type at the focused pane: the marker lands in one of the two
    // shells; `terminal.read` on each name tells which.
    let first = type_until_read(
        &tui,
        &mut vt,
        &client,
        &["smoke-a", "smoke-b"],
        b"echo AAA1\r",
        "AAA1",
    )
    .await;
    let second = if first == "smoke-a" {
        "smoke-b"
    } else {
        "smoke-a"
    };
    until_screen(
        &tui,
        &mut vt,
        |v| screen(v).contains("AAA1"),
        "echo in pane",
    )
    .await;

    // 4. Cycle panes: the next marker must land in the other shell.
    tui.type_keys(b"\x02\x09"); // prefix+tab
    settle(&tui, &mut vt, Duration::from_millis(300)).await;
    let landed = type_until_read(
        &tui,
        &mut vt,
        &client,
        &["smoke-a", "smoke-b"],
        b"echo BBB2\r",
        "BBB2",
    )
    .await;
    assert_eq!(landed, second, "cycle did not move focus to the other pane");

    // 5. New tab (name prompt takes Enter) and a split each create a
    // fresh terminal on the core.
    tui.type_keys(b"\x02c\r"); // prefix+c, confirm the name
    let deadline = Instant::now() + DEADLINE;
    while terminal_count(&client).await < 3 {
        assert!(
            Instant::now() < deadline,
            "new tab never created a terminal; screen was:\n{}",
            screen(&vt)
        );
        tui.pump(&mut vt);
        tokio::time::sleep(POLL).await;
    }
    settle(&tui, &mut vt, Duration::from_millis(400)).await;
    tui.type_keys(b"\x02v"); // prefix+v
    let deadline = Instant::now() + DEADLINE;
    while terminal_count(&client).await < 4 {
        assert!(
            Instant::now() < deadline,
            "split never created a terminal; screen was:\n{}",
            screen(&vt)
        );
        tui.pump(&mut vt);
        tokio::time::sleep(POLL).await;
    }

    // 6. OSC 7501: the report's `app` field surfaces as the sidebar
    // label — `display_agent` replaces `name` on either core.
    run(
        &scratch,
        &client,
        "smoke-osc",
        reporter("working:app=smokeapp"),
    )
    .await;
    until_screen(
        &tui,
        &mut vt,
        |v| screen(v).contains("smokeapp"),
        "OSC 7501 status in the sidebar",
    )
    .await;

    // 7. Detach: clean exit, restored modes.
    tui.type_keys(b"\x02q"); // prefix+q
    let deadline = Instant::now() + DEADLINE;
    let status = loop {
        match tui.child.try_wait().expect("try_wait") {
            Some(status) => break status,
            None => {
                assert!(
                    Instant::now() < deadline,
                    "hc-tui did not exit after detach"
                );
                tokio::time::sleep(POLL).await;
            }
        }
    };
    assert!(status.success(), "hc-tui exited with {status}");
    let mut modes: libc::termios = unsafe { std::mem::zeroed() };
    unsafe {
        assert_eq!(libc::tcgetattr(tui.slave.as_raw_fd(), &mut modes), 0);
    }
    assert_eq!(
        (modes.c_lflag, modes.c_iflag, modes.c_oflag),
        (
            tui.initial_modes.c_lflag,
            tui.initial_modes.c_iflag,
            tui.initial_modes.c_oflag
        ),
        "terminal modes were not restored on detach"
    );

    drop(client);
    drop(tui);
    scratch.finish().expect("scratch teardown");
}

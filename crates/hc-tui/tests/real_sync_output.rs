//! Opt-in live check: a program redrawing a block at ~30 fps inside
//! synchronized-output frames must never reach the screen half written.
//!
//! hc-tui wraps its own host output in `CSI ? 2026 h/l` (ansi.rs), so the
//! PTY stream splits into the client's paints. Each paint is fed into a
//! VT in order; a paint that shows a frame's first line must also show the
//! rest of it.
//!
//! Ignored by default. Run with
//!
//! ```sh
//! HC_TUI_TEST_CORE=/path/to/pinned/herdr-core \
//!     cargo test -p hc-tui --test real_sync_output -- --ignored --nocapture
//! ```

#![cfg(unix)]

#[path = "../../hc-proto/tests/common/scratch.rs"]
mod scratch;

use hc_proto::client::Client;
use hc_proto::endpoint::Endpoint;
use hc_proto::terminal::RunParams;
use hc_vt::Vt;
use scratch::ScratchCore;

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Geometry big enough for sidebar plus a usable pane.
const COLS: u16 = 120;
const ROWS: u16 = 34;
/// Finite bound on the whole run.
const DEADLINE: Duration = Duration::from_secs(30);
/// Read pump cadence.
const POLL: Duration = Duration::from_millis(20);
/// Paints to judge — enough that a 30 fps program is well sampled.
const PAINTS: usize = 8;

const BEGIN: &[u8] = b"\x1b[?2026h";
const END: &[u8] = b"\x1b[?2026l";

/// The `hc-tui` binary under test.
fn tui_bin() -> &'static str {
    env!("CARGO_BIN_EXE_hc-tui")
}

/// The fixture's own environment, nothing inherited.
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

/// A pty pair at `cols`x`rows`, master nonblocking.
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

/// `hc-tui` hosted in a pty: own session, controlling terminal.
fn spawn_tui(scratch: &ScratchCore) -> (OwnedFd, Child) {
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
    (master, child)
}

/// Splits the host stream into the client's own paints, which are the
/// bytes between its `2026h` and `2026l`.
#[derive(Default)]
struct Paints {
    pending: Vec<u8>,
}

impl Paints {
    fn push(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            let Some(begin) = find(&self.pending, BEGIN, 0) else {
                // Nothing open: everything but a possible split marker
                // belongs to the startup paint.
                if self.pending.len() > BEGIN.len() - 1 {
                    let keep = BEGIN.len() - 1;
                    let split = self.pending.len() - keep;
                    out.push(self.pending.drain(..split).collect());
                }
                break;
            };
            let Some(end) = find(&self.pending, END, begin) else {
                // An open paint: wait for its close.
                if begin > 0 {
                    out.push(self.pending.drain(..begin).collect());
                }
                break;
            };
            let end = end + END.len();
            out.push(self.pending.drain(..end).collect());
        }
        out
    }
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if haystack.len() < needle.len() || from > haystack.len() - needle.len() {
        return None;
    }
    (from..=haystack.len() - needle.len()).find(|at| &haystack[*at..*at + needle.len()] == needle)
}

/// The emulated screen as text rows.
fn screen(vt: &mut Vt) -> String {
    let (cols, rows) = vt.size().expect("vt size");
    (0..rows)
        .map(|row| vt.screen_row_text(row.into(), cols).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A program that repaints a three-line block inside a synchronized frame
/// at ~30 fps, pausing between the first and the rest of the frame so the
/// core has to deliver it in pieces.
fn repainter() -> Vec<String> {
    vec![
        "sh".into(),
        "-c".into(),
        concat!(
            "n=0; while :; do ",
            "printf '\\033[?2026h\\033[H\\033[2JFRAME %03d A\\r\\n' \"$n\"; ",
            "printf '\\033[8;1H'; sleep 0.004; ",
            "printf 'FRAME %03d B\\r\\nFRAME %03d C\\033[?2026l' \"$n\" \"$n\"; ",
            "sleep 0.03; n=$((n+1)); done"
        )
        .into(),
    ]
}

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn every_paint_shows_a_complete_synchronized_frame() {
    let scratch = ScratchCore::spawn().await;
    let client = Client::connect(&scratch.endpoint, None::<&std::ffi::OsStr>)
        .await
        .expect("connect to scratch core");
    client
        .terminal_run(RunParams {
            argv: repainter(),
            name: Some("sync-frames".into()),
            cwd: Some(scratch.work.to_string_lossy().into_owned()),
            ..RunParams::default()
        })
        .await
        .expect("terminal.run");

    let (master, mut child) = spawn_tui(&scratch);
    let mut paints = Paints::default();
    let mut vt = Vt::new(COLS, ROWS, 0);
    let mut judged = 0usize;
    let mut torn: Vec<String> = Vec::new();
    let mut buf = [0u8; 65536];
    let deadline = Instant::now() + DEADLINE;
    while judged < PAINTS && Instant::now() < deadline {
        let n = unsafe { libc::read(master.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if n <= 0 {
            tokio::time::sleep(POLL).await;
            continue;
        }
        for paint in paints.push(&buf[..n as usize]) {
            vt.feed(&paint);
            let screen = screen(&mut vt);
            // Only paints that show a frame's first line are judged.
            let Some(number) = screen
                .split("FRAME ")
                .nth(1)
                .and_then(|rest| rest.split_whitespace().next().map(str::to_owned))
            else {
                continue;
            };
            if !screen.contains("FRAME ") || !screen.contains(" A") {
                continue;
            }
            judged += 1;
            let complete = screen.contains(&format!("FRAME {number} A"))
                && screen.contains(&format!("FRAME {number} B"))
                && screen.contains(&format!("FRAME {number} C"));
            if !complete {
                torn.push(screen.clone());
            }
        }
        // The emulated terminal owes replies (queries, reports).
        let replies = vt.take_replies().unwrap_or_default();
        if !replies.is_empty() {
            unsafe {
                libc::write(master.as_raw_fd(), replies.as_ptr().cast(), replies.len());
            }
        }
    }
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    let _ = child.wait();

    println!("{judged} paints judged, {} torn", torn.len());
    assert!(
        judged >= PAINTS,
        "the program never painted: screen states = {judged}"
    );
    assert!(
        torn.is_empty(),
        "a paint showed a half-written frame:\n{}",
        torn.first().cloned().unwrap_or_default()
    );
}

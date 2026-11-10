//! Opt-in real-core end-to-end for the R9 image-paste bridge: the actual
//! `hc-tui --remote` binary in a PTY, a fake `ssh` that runs
//! `hc connect` locally, and a fake `wl-paste` serving a small PNG or
//! a copied-image file list. `Ctrl+V` — typed as the real kitty
//! press/release CSI-u stream — must upload through the real core's
//! `terminal.put` and the returned path must reach the focused
//! program — the file it names must exist.
//!
//! Ignored by default: `cargo test` never touches a real core. Run
//! explicitly with
//!
//! ```sh
//! HC_TUI_TEST_CORE=/path/to/pinned/herdr-core \
//!     cargo test -p hc-tui --test image_paste_e2e -- --ignored --test-threads=1
//! ```
//!
//! `HC_TUI_TEST_CORE` must name a pinned build (see
//! `tools/build-test-core.sh`), never the installed `hc`: the fixture
//! spawns it as the scratch core.

#![cfg(unix)]

#[path = "../../hc-proto/tests/common/scratch.rs"]
mod scratch;

use scratch::ScratchCore;

use hc_proto::client::Client;
use hc_proto::endpoint::Endpoint;
use hc_proto::terminal::RunParams;
use hc_vt::Vt;

use std::collections::BTreeMap;
use std::fs;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const COLS: u16 = 120;
const ROWS: u16 = 34;
const DEADLINE: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(40);

/// A tiny valid PNG (8-byte signature plus one IHDR-shaped tail — the
/// bridge checks the signature, the core stores bytes verbatim).
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\x00\x00\x00\x01\x00\x00\x00\x01";

fn tui_bin() -> &'static str {
    env!("CARGO_BIN_EXE_backlot")
}

/// Write a PATH stub and probe-exec it until the file is reliably
/// executable: a fork elsewhere in the process can briefly pin the
/// just-written script's writer fd and a racing `exec` fails with
/// ETXTBSY. The `STUB_PROBE` line makes each probe run a no-op.
fn write_executable(path: &std::path::Path, body: &str) {
    fs::write(
        path,
        format!("#!/bin/sh\n[ -n \"$STUB_PROBE\" ] && exit 0\n{body}"),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    for _ in 0..200 {
        match Command::new(path)
            .env("STUB_PROBE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Ok(_) => return,
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => panic!("cannot exec stub {}: {e}", path.display()),
        }
    }
    panic!("stub {} never became executable", path.display());
}

/// The environment `hc-tui --remote` sees: the scratch fixture's own
/// plus the stub dir ahead of PATH and a Wayland display so the
/// `wl-paste` reader arm is selected.
fn tui_env(scratch: &ScratchCore, stub: &std::path::Path) -> Vec<(String, String)> {
    let root = |dir: &str| scratch.root.join(dir).to_string_lossy().into_owned();
    let Endpoint::Local(local) = &scratch.endpoint else {
        panic!("scratch endpoint is always local");
    };
    vec![
        ("PATH".into(), format!("{}:/usr/bin:/bin", stub.display())),
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
        ("WAYLAND_DISPLAY".into(), "wayland-0".into()),
    ]
}

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

struct Tui {
    master: OwnedFd,
    slave: OwnedFd,
    initial_modes: libc::termios,
    child: Child,
}

fn spawn_tui(scratch: &ScratchCore, stub: &std::path::Path) -> Tui {
    let (master, slave) = openpty(COLS, ROWS);
    let mut initial_modes: libc::termios = unsafe { std::mem::zeroed() };
    unsafe {
        assert_eq!(libc::tcgetattr(slave.as_raw_fd(), &mut initial_modes), 0);
    }
    let sfd = slave.as_raw_fd();
    let dup = || unsafe { Stdio::from_raw_fd(libc::dup(sfd)) };
    let mut command = Command::new(tui_bin());
    command
        .arg("--remote")
        .arg("host-a")
        .env_clear()
        .envs(tui_env(scratch, stub))
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
    Tui {
        master,
        slave,
        initial_modes,
        child,
    }
}

impl Drop for Tui {
    /// A panic mid-test must not leave the TUI running: the orphaned
    /// child keeps inherited fds — including a caller's lock — alive.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Tui {
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

fn screen(vt: &Vt) -> String {
    let (cols, rows) = vt.size().expect("vt size");
    (0..rows)
        .map(|row| vt.screen_row_text(row.into(), cols).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

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

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn ctrl_v_uploads_the_clipboard_image_over_remote() {
    let scratch = ScratchCore::spawn().await;
    let png_path = scratch.root.join("clip.png");
    fs::write(&png_path, PNG).unwrap();
    image_paste_e2e(
        scratch,
        &format!(
            "case \"$2\" in image/png) cat \"{}\" ;; *) exit 1 ;; esac\n",
            png_path.display()
        ),
    )
    .await;
}

/// A copied image FILE offers `x-special/gnome-copied-files`, never an
/// image MIME: the bridge must resolve the named local file and upload
/// its bytes. Pre-fix this read answered `None` and `Ctrl+V` forwarded
/// silently.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn ctrl_v_uploads_a_copied_image_file_over_remote() {
    let scratch = ScratchCore::spawn().await;
    let png_path = scratch.root.join("copied.png");
    fs::write(&png_path, PNG).unwrap();
    image_paste_e2e(
        scratch,
        &format!(
            "case \"$2\" in x-special/gnome-copied-files) printf 'copy\\nfile://%s\\n' \"{}\" ;; *) exit 1 ;; esac\n",
            png_path.display()
        ),
    )
    .await;
}

/// The shared end-to-end: `wl_paste_body` is the fake `wl-paste` case
/// script (`$2` holds the requested MIME). Takes ownership so teardown
/// can consume the scratch core at the end.
async fn image_paste_e2e(scratch: ScratchCore, wl_paste_body: &str) {
    let client = connect(&scratch).await;

    // The stub dir: `hc` (the pinned binary), a fake `ssh` that runs the
    // remote command locally, and a fake `wl-paste` serving the image.
    let stub = scratch.root.join("stub");
    fs::create_dir(&stub).unwrap();
    std::os::unix::fs::symlink(scratch::core_binary(), stub.join("hc")).unwrap();
    // The fake `ssh`: run the remote command (its last argument)
    // locally, with the protocol stream passing straight through.
    write_executable(
        &stub.join("ssh"),
        "for last; do :; done\nexec sh -c \"$last\"\n",
    );
    write_executable(&stub.join("wl-paste"), wl_paste_body);

    // The stand-in program: writes its stdin to a file so the pasted
    // path lands somewhere checkable.
    let received = scratch.work.join("received.txt");
    client
        .terminal_run(RunParams {
            argv: vec![
                "sh".into(),
                "-c".into(),
                format!("cat > \"{}\"", received.display()),
            ],
            name: Some("standin".into()),
            cwd: Some(scratch.work.to_string_lossy().into_owned()),
            labels: BTreeMap::from([("herdr.seen".into(), "0".into())]),
            ..RunParams::default()
        })
        .await
        .expect("terminal.run standin");

    let mut tui = spawn_tui(&scratch, &stub);
    let mut vt = Vt::new(COLS, ROWS, 4096);
    until_screen(
        &tui,
        &mut vt,
        |v| screen(v).contains("agents"),
        "initial UI render",
    )
    .await;

    /// Poll `received.txt` until `needle` appears — the focused pane's
    /// `cat` is the only writer, so content proves input delivery.
    async fn until_received(
        tui: &Tui,
        vt: &mut Vt,
        received: &std::path::Path,
        needle: &str,
        what: &str,
    ) {
        let deadline = Instant::now() + DEADLINE;
        loop {
            if fs::read_to_string(received)
                .ok()
                .is_some_and(|t| t.contains(needle))
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{what} never reached the program; screen was:\n{}",
                screen(vt)
            );
            tui.pump(vt);
            tokio::time::sleep(POLL).await;
        }
    }

    // The attach is live when typed keys land in cat's file.
    tui.type_keys(b"ready\n");
    until_received(&tui, &mut vt, &received, "ready", "typed input").await;

    // A bracketed plain-text paste stays text and lands in order.
    tui.type_keys(b"\x1b[200~hello-text\n\x1b[201~");
    until_received(&tui, &mut vt, &received, "hello-text", "text paste").await;

    // Ctrl+V is the default keys.remote_image_paste trigger: the fake
    // wl-paste's image uploads through the real `terminal.put` and the
    // returned path is pasted. The keys are the real kitty stream —
    // `CSI 118;5u` press then `CSI 118;5:3u` release under the pushed
    // enhancement flags (the host Vt answers `CSI ?u`, so release
    // suppression is armed). The paste adds no newline, so the path
    // waits in the PTY's canonical buffer until Enter flushes it and
    // cat writes it as the last line.
    tui.type_keys(b"\x1b[118;5u\x1b[118;5:3u");
    let deadline = Instant::now() + DEADLINE;
    loop {
        // The path wraps mid-word on the small pane; joining the text
        // right of the sidebar separator restores it.
        let pane: String = screen(&vt)
            .lines()
            .filter_map(|line| line.split_once('│').map(|(_, pane)| pane))
            .collect();
        if pane.contains("files/t1/clipboard-") {
            tui.type_keys(b"\r");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the uploaded path never reached the pane; screen was:\n{}",
            screen(&vt)
        );
        tui.pump(&mut vt);
        tokio::time::sleep(POLL).await;
    }
    let path = {
        let deadline = Instant::now() + DEADLINE;
        loop {
            tui.pump(&mut vt);
            let candidate = fs::read_to_string(&received)
                .ok()
                .and_then(|t| {
                    t.rsplit_once("hello-text\n")
                        .map(|(_, tail)| tail.trim().to_owned())
                })
                .filter(|tail| tail.starts_with('/'));
            if let Some(candidate) = candidate
                && std::path::Path::new(&candidate).exists()
            {
                break candidate;
            }
            assert!(
                Instant::now() < deadline,
                "the pasted path never reached the program; received.txt was {:?}; screen was:\n{}",
                fs::read_to_string(&received).unwrap_or_default(),
                screen(&vt)
            );
            tokio::time::sleep(POLL).await;
        }
    };
    assert_eq!(
        fs::read(&path).expect("read the uploaded image"),
        PNG,
        "uploaded bytes round-trip"
    );

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

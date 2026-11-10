//! Shared PTY harness for the paste-matrix e2e: drives the real `hc-tui`
//! binary on a scratch core with everything the test touches owned by the
//! test — the PTY, the stub `ssh`, the stub clipboard tools and the fake
//! agent programs. The real user clipboard and any real session are never
//! reached. Modelled on `image_paste_e2e.rs`.

#![allow(dead_code)]

use std::fs;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use hc_vt::Vt;

pub const COLS: u16 = 120;
pub const ROWS: u16 = 34;
pub const DEADLINE: Duration = Duration::from_secs(20);
pub const POLL: Duration = Duration::from_millis(40);

/// A tiny valid PNG (8-byte signature plus an IHDR-shaped tail — the
/// reader checks the signature, the core stores bytes verbatim).
pub const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\x00\x00\x00\x01\x00\x00\x00\x01";

/// A minimal JPEG — signature plus a JFIF APP0 stub; type detection only
/// reads the magic bytes.
pub const JPEG: &[u8] = b"\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01\x01\xff\xd9";

/// Writes `path` as an executable script and probe-execs it until exec
/// works: a racing fork elsewhere can pin the writer fd and an immediate
/// `exec` fails with ETXTBSY (same fix the image e2e uses).
pub fn write_executable(path: &Path, body: &str) {
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

/// The running TUI plus its host-terminal side. `raw` is the byte tap for
/// OSC 52 inspection; `vt` decodes what a real terminal would show and
/// auto-answers the app's queries (DA, kitty `?u`) via `take_replies`.
pub struct Tui {
    master: OwnedFd,
    slave: OwnedFd,
    initial_modes: libc::termios,
    pub vt: Vt,
    pub raw: Vec<u8>,
    osc52_mark: usize,
    pub child: Child,
}

/// Spawns `hc-tui` under `login_tty` with the scratch env plus `extra_env`
/// (stub PATH dirs, display vars) and `args` (`--remote host-a` or none).
pub fn spawn_tui(env: &[(String, String)], args: &[String], cwd: &Path) -> Tui {
    let (master, slave) = openpty(COLS, ROWS);
    let mut initial_modes: libc::termios = unsafe { std::mem::zeroed() };
    unsafe {
        assert_eq!(libc::tcgetattr(slave.as_raw_fd(), &mut initial_modes), 0);
    }
    let sfd = slave.as_raw_fd();
    let dup = || unsafe { Stdio::from_raw_fd(libc::dup(sfd)) };
    let bin = env!("CARGO_BIN_EXE_hc-tui");
    let mut command = Command::new(bin);
    command
        .args(args)
        .env_clear()
        .envs(env.iter().cloned())
        .env("TERM", "xterm-256color")
        .env("HERDR_UI_TEST", "1")
        .current_dir(cwd)
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
        .unwrap_or_else(|e| panic!("cannot spawn {bin}: {e}"));
    Tui {
        master,
        slave,
        initial_modes,
        vt: Vt::new(COLS, ROWS, 4096),
        raw: Vec::new(),
        osc52_mark: 0,
        child,
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Tui {
    /// Drains the PTY into the decoder and the raw tap, then writes the
    /// terminal's queued replies (DA, kitty query) back like a real
    /// terminal would.
    pub fn pump(&mut self) {
        let mut buf = [0u8; 65536];
        loop {
            let n =
                unsafe { libc::read(self.master.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                break;
            }
            self.vt.feed(&buf[..n as usize]);
            self.raw.extend_from_slice(&buf[..n as usize]);
        }
        let replies = self.vt.take_replies().unwrap_or_default();
        if !replies.is_empty() {
            self.type_keys(&replies);
        }
    }

    /// Types host bytes — a kitty CSI-u stream, raw bytes, or a bracketed
    /// paste. Retries on EAGAIN: the master is nonblocking and a big
    /// paste overruns the PTY input buffer while the app drains it.
    pub fn type_keys(&self, keys: &[u8]) {
        let mut written = 0;
        let deadline = Instant::now() + Duration::from_secs(20);
        while written < keys.len() {
            let n = unsafe {
                libc::write(
                    self.master.as_raw_fd(),
                    keys[written..].as_ptr().cast(),
                    keys.len() - written,
                )
            };
            if n > 0 {
                written += n as usize;
                continue;
            }
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            let alive = std::path::Path::new(&format!("/proc/{}", self.child.id())).exists();
            panic!(
                "pty write failed ({err}, child {} alive={}); screen was:\n{}",
                self.child.id(),
                alive,
                screen(&self.vt)
            );
        }
    }

    /// A bracketed paste — how a terminal delivers `Ctrl+Shift+V` text.
    pub fn paste_text(&self, text: &str) {
        self.type_keys(b"\x1b[200~");
        self.type_keys(text.as_bytes());
        self.type_keys(b"\x1b[201~");
    }

    /// SGR mouse event; coords are 1-based cells. `release` sends the `m`
    /// final byte.
    pub fn mouse(&self, cb: u8, col: u16, row: u16, release: bool) {
        let seq = if release {
            format!("\x1b[<{cb};{col};{row}m")
        } else {
            format!("\x1b[<{cb};{col};{row}M")
        };
        self.type_keys(seq.as_bytes());
    }

    /// A plain left click (press + release, same cell).
    pub fn click(&self, col: u16, row: u16) {
        self.mouse(0, col, row, false);
        self.mouse(0, col, row, true);
    }

    /// A left drag selecting text from (c1,r1) to (c2,r2).
    pub fn drag(&self, c1: u16, r1: u16, c2: u16, r2: u16) {
        self.mouse(0, c1, r1, false);
        let steps: i32 = 4;
        for i in 1..=steps {
            let col = c1 as i32 + (c2 as i32 - c1 as i32) * i / steps;
            let row = r1 as i32 + (r2 as i32 - r1 as i32) * i / steps;
            self.mouse(32, col.max(1) as u16, row.max(1) as u16, false);
        }
        self.mouse(0, c2, r2, true);
    }

    pub fn screen(&mut self) -> String {
        self.pump();
        screen(&self.vt)
    }

    /// First 1-based `(col,row)` where `needle` appears on screen — for
    /// clicking sidebar rows and pane content. `col` counts cells, not
    /// bytes: box-drawing and CJK chars inflate byte offsets.
    pub fn find_text(&mut self, needle: &str) -> Option<(u16, u16)> {
        let s = self.screen();
        for (row, line) in s.lines().enumerate() {
            if let Some(byte_col) = line.find(needle) {
                let cells = line[..byte_col].chars().count();
                return Some((cells as u16 + 1, row as u16 + 1));
            }
        }
        None
    }

    /// Decoded payloads of `OSC 52 ;c;` writes since the last call — the
    /// "terminal clipboard" the test owns.
    pub fn take_osc52(&mut self) -> Vec<Vec<u8>> {
        self.pump();
        let mut out = Vec::new();
        let mut rest = &self.raw[self.osc52_mark.min(self.raw.len())..];
        let mut rel = 0usize;
        while let Some(pos) = rest.windows(5).position(|w| w == b"\x1b]52;") {
            let after = &rest[pos + 5..];
            let Some(end) = after.iter().position(|&b| b == 0x07) else {
                break;
            };
            let payload = &after[..end];
            rel = pos + 5 + end + 1;
            rest = &after[end + 1..];
            if let Some(b64) = payload.strip_prefix(b"c;")
                && let Ok(bytes) = decode_b64(b64)
            {
                out.push(bytes);
            }
        }
        self.osc52_mark += rel;
        out
    }

    /// Drops the captured raw tail — used between cells so one cell's
    /// output never feeds another's checks. Resets the OSC52 scan mark:
    /// a mark past the (now shorter) buffer would hide later copies.
    pub fn clear_raw(&mut self) {
        self.raw.clear();
        self.osc52_mark = 0;
    }

    /// Detach with prefix+q and wait for exit; asserts modes restored.
    pub async fn detach(&mut self) {
        self.type_keys(b"\x02q");
        let deadline = Instant::now() + DEADLINE;
        let status = loop {
            match self.child.try_wait().expect("try_wait") {
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
            assert_eq!(libc::tcgetattr(self.slave.as_raw_fd(), &mut modes), 0);
        }
        assert_eq!(
            (modes.c_lflag, modes.c_iflag, modes.c_oflag),
            (
                self.initial_modes.c_lflag,
                self.initial_modes.c_iflag,
                self.initial_modes.c_oflag
            ),
            "terminal modes were not restored on detach"
        );
    }
}

/// Waits until `pred` holds on the decoded screen, with the raw tap
/// available to the predicate. Returns false on timeout.
pub async fn until(tui: &mut Tui, pred: impl Fn(&Vt, &[u8]) -> bool, _what: &str) -> bool {
    try_until(tui, DEADLINE, pred).await
}

/// [`until`] with an explicit timeout.
pub async fn try_until(
    tui: &mut Tui,
    timeout: Duration,
    pred: impl Fn(&Vt, &[u8]) -> bool,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        tui.pump();
        if pred(&tui.vt, &tui.raw) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(POLL).await;
    }
}

/// Waits until `pred` holds on the filesystem (a fake's `received` file).
pub async fn until_file(tui: &mut Tui, path: &Path, pred: impl Fn(&[u8]) -> bool, what: &str) {
    let deadline = Instant::now() + DEADLINE;
    loop {
        if fs::read(path).ok().is_some_and(|b| pred(&b)) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{what}; screen was:\n{}",
            screen(&tui.vt)
        );
        tui.pump();
        tokio::time::sleep(POLL).await;
    }
}

pub fn screen(vt: &Vt) -> String {
    let (cols, rows) = vt.size().expect("vt size");
    (0..rows)
        .map(|row| vt.screen_row_text(row.into(), cols).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Base64 decode for OSC 52 payloads (no dependency).
fn decode_b64(b64: &[u8]) -> Result<Vec<u8>, ()> {
    let table = |b: u8| -> Option<u32> {
        match b {
            b'A'..=b'Z' => Some((b - b'A') as u32),
            b'a'..=b'z' => Some((b - b'a' + 26) as u32),
            b'0'..=b'9' => Some((b - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let mut out = Vec::with_capacity(b64.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for &b in b64 {
        if b == b'=' {
            break;
        }
        let Some(v) = table(b) else { continue };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

/// The environment `hc-tui` sees for a local-connect run: scratch XDG/HOME
/// roots plus the stub dir ahead of PATH. `display` adds a Wayland hint so
/// the `wl-paste` reader arm is exercised.
pub fn env_base(scratch_root: &Path, session: &str, socket: &Path) -> Vec<(String, String)> {
    let root = |dir: &str| scratch_root.join(dir).to_string_lossy().into_owned();
    vec![
        ("PATH".into(), "/usr/bin:/bin".into()),
        ("LANG".into(), "C.UTF-8".into()),
        ("HOME".into(), root("home")),
        ("XDG_STATE_HOME".into(), root("state")),
        ("XDG_CONFIG_HOME".into(), root("config")),
        ("XDG_RUNTIME_DIR".into(), root("run")),
        ("HC_SESSION".into(), session.into()),
        ("HC_SOCKET".into(), socket.to_string_lossy().into_owned()),
        ("WAYLAND_DISPLAY".into(), "wayland-0".into()),
    ]
}

/// File-backed clipboard stub: `wl-paste -l` lists offered MIME types
/// (files under `dir/mime/<name with _ for />`), `wl-paste -t mime` writes
/// the payload. Also usable as `xclip` via the same arg shape.
pub fn clipboard_stub_body(dir: &Path) -> String {
    format!(
        "dir='{dir}'\n\
         list=0; want=''\n\
         while [ $# -gt 0 ]; do case \"$1\" in\n\
         -l|--list-types|-t) : ;;\n\
         TARGETS) list=1 ;;\n\
         -n|--no-newline) : ;;\n\
         -o|-selection|-loops|-verbose|-filter) shift 2>/dev/null ;;\n\
         clipboard|primary) : ;;\n\
         image/*|x-special/*|text/*|application/*) want=\"$1\" ;;\n\
         esac; shift; done\n\
         for last; do :; done\n\
         case \"$last\" in image/*|x-special/*|text/*|application/*) want=\"$last\";; esac\n\
         [ $list -eq 1 ] && {{ ls \"$dir/mime\" 2>/dev/null; exit 0; }}\n\
         [ -n \"$want\" ] || exit 0\n\
         f=\"$dir/mime/$(printf %s \"$want\" | tr / _)\"\n\
         [ -f \"$f\" ] && cat \"$f\"\n",
        dir = dir.display()
    )
}

/// Replace the whole stub clipboard content — a real clipboard holds one
/// selection, so leftover types from an earlier set would be picked up
/// first (image/png wins over image/jpeg in the reader's order).
pub fn clipboard_set(dir: &Path, mime: &str, data: &[u8]) {
    let mime_dir = dir.join("mime");
    let _ = fs::remove_dir_all(&mime_dir);
    fs::create_dir_all(&mime_dir).unwrap();
    fs::write(mime_dir.join(mime.replace('/', "_")), data).unwrap();
}

pub fn clipboard_clear(dir: &Path) {
    let _ = fs::remove_dir_all(dir.join("mime"));
}

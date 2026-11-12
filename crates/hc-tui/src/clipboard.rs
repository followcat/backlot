// Derived from herdr src/client/clipboard_images.rs and
// src/platform/{mod,linux,macos}.rs (Apache-2.0).
//! Local clipboard-image reads and dropped-image-path parsing for the
//! remote image-paste bridge (R9). Unlike the reference's unbounded
//! reads, every spawned tool here is killed after about two seconds:
//! a hung clipboard owner must not stall input forwarding, and a
//! missing tool reports "unavailable", never a silent "no image".

use std::ffi::OsString;
use std::io::Read;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::{Command, Stdio};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::sync::mpsc;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::thread;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::time::Duration;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::time::Instant;

/// `terminal.put` accepts at most 8 MB of raw data — the bridge bound.
pub(crate) const MAX_CLIPBOARD_IMAGE_BYTES: usize = 8 * 1024 * 1024;

/// About how long one clipboard read may run before its owner is
/// declared hung. Kept a named constant so tests can shrink it.
#[cfg(any(target_os = "linux", target_os = "macos"))]
const READER_TIMEOUT: Duration = Duration::from_secs(2);

/// One image read off the local clipboard or a dropped file.
#[derive(Debug)]
pub(crate) struct ClipboardImage {
    pub bytes: Vec<u8>,
    pub extension: &'static str,
}

/// What a clipboard or dropped-file read produced: nothing usable
/// (`None`), an image under the `terminal.put` bound (`Image`), or a
/// payload over the bound whose capped prefix still matched the
/// advertised format (`TooLarge`). `Unavailable` is the "can't even
/// try" arm — no display, every tool missing, or a hung owner — and
/// carries the reason a notice shows the user. The bridge reports
/// `TooLarge` instead of forwarding the gesture as plain input; `None`
/// and `Unavailable` forward it, but never silently.
#[derive(Debug)]
pub(crate) enum ClipboardRead {
    None,
    Unavailable(&'static str),
    Image(ClipboardImage),
    TooLarge,
}

/// The reader-selection facts `read_clipboard_image` needs; tests inject
/// fakes so they never touch a real clipboard. Display-selection fields
/// exist only where a reader consults them.
#[derive(Debug, Default)]
pub(crate) struct ClipboardEnv {
    /// Inside WSL: `powershell.exe` answers the Windows clipboard first.
    #[cfg(target_os = "linux")]
    pub wsl: bool,
    /// `WAYLAND_DISPLAY` is set: `wl-paste` may answer.
    #[cfg(target_os = "linux")]
    pub wayland: bool,
    /// `DISPLAY` is set: `xclip` may answer.
    #[cfg(target_os = "linux")]
    pub display: bool,
    /// PATH the spawned tools resolve against. `None` inherits the
    /// process's own; tests pass a directory of stubs.
    pub path: Option<OsString>,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl ClipboardEnv {
    fn current() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            wsl: running_inside_wsl(),
            #[cfg(target_os = "linux")]
            wayland: std::env::var_os("WAYLAND_DISPLAY").is_some(),
            #[cfg(target_os = "linux")]
            display: std::env::var_os("DISPLAY").is_some(),
            path: None,
        }
    }
}

/// The image on the local clipboard, if one is offered in a supported
/// format. A missing tool, an empty clipboard, an invalid payload and a
/// timed-out owner all read as `None`; a payload over the bound with a
/// matching signature prefix reads as `TooLarge`.
pub(crate) fn read_clipboard_image() -> ClipboardRead {
    #[cfg(target_os = "linux")]
    {
        read_linux_clipboard_image(&ClipboardEnv::current(), READER_TIMEOUT)
    }
    #[cfg(target_os = "macos")]
    {
        read_macos_clipboard_image(&ClipboardEnv::current(), READER_TIMEOUT)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        ClipboardRead::Unavailable("This platform has no clipboard image reader")
    }
}

/// The bounded-read outcome of one spawned tool. `Oversized` keeps the
/// capped prefix so a caller's signature check can tell "too large"
/// apart from "not an image".
pub(crate) enum LimitedRead {
    Empty,
    Complete(Vec<u8>),
    Oversized(Vec<u8>),
}

/// The reference `read_limited_reader`: at most `max_bytes`, then one
/// sentinel byte decides `Complete` vs `Oversized`.
pub(crate) fn read_limited(
    mut reader: impl Read,
    max_bytes: usize,
) -> std::io::Result<LimitedRead> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    while bytes.len() < max_bytes {
        let read_len = (max_bytes - bytes.len()).min(buffer.len());
        let n = match reader.read(&mut buffer[..read_len]) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if n == 0 {
            return Ok(if bytes.is_empty() {
                LimitedRead::Empty
            } else {
                LimitedRead::Complete(bytes)
            });
        }
        bytes.extend_from_slice(&buffer[..n]);
    }
    let mut sentinel = [0u8; 1];
    loop {
        return match reader.read(&mut sentinel) {
            Ok(0) if bytes.is_empty() => Ok(LimitedRead::Empty),
            Ok(0) => Ok(LimitedRead::Complete(bytes)),
            Ok(_) => Ok(LimitedRead::Oversized(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => Err(e),
        };
    }
}

/// What one bounded tool run produced. `TimedOut` marks that tool
/// hung: its later calls are skipped so the wait stays bounded, but
/// the next tool still runs — a dead display cannot hide an image on
/// another one. `Oversized` keeps the capped prefix for the caller's
/// signature check. `Missing` means the tool could not even be
/// spawned — the caller counts it separately so "no tool exists"
/// never reports as "no image".
#[cfg(any(target_os = "linux", target_os = "macos"))]
enum ReaderOutcome {
    Data(Vec<u8>),
    Oversized(Vec<u8>),
    Empty,
    TimedOut,
    Missing,
}

/// Spawn a clipboard tool and drain its stdout, bounded in bytes and
/// time. A missing tool is `Missing`, a nonzero exit and an empty
/// payload are `Empty`; a deadline kill is `TimedOut`; a stream that
/// filled the cap is `Oversized` with the prefix retained.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn read_tool(mut command: Command, timeout: Duration) -> ReaderOutcome {
    let mut child = match command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return ReaderOutcome::Missing,
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return ReaderOutcome::Empty;
    };
    // The drain lives on its own thread so the deadline also applies to
    // a blocked read(), not just a still-running child.
    let (send, recv) = mpsc::channel();
    let drain = thread::spawn(move || {
        let _ = send.send(read_limited(stdout, MAX_CLIPBOARD_IMAGE_BYTES));
    });
    let deadline = Instant::now() + timeout;
    let outcome = loop {
        match recv.try_recv() {
            Ok(result) => break result.unwrap_or(LimitedRead::Empty),
            Err(mpsc::TryRecvError::Disconnected) => break LimitedRead::Empty,
            Err(mpsc::TryRecvError::Empty) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    // A dead shell may leave a grandchild holding the
                    // pipe — detach the drain instead of joining it.
                    drop(drain);
                    return ReaderOutcome::TimedOut;
                }
                thread::sleep(Duration::from_millis(2));
            }
        }
    };
    if let LimitedRead::Oversized(prefix) = outcome {
        let _ = child.kill();
        let _ = child.wait();
        let _ = drain.join();
        return ReaderOutcome::Oversized(prefix);
    }
    // stdout closed: give the child the rest of the deadline to exit,
    // then take its status like the reference does.
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(2)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Err(_) => break None,
        }
    };
    let _ = drain.join();
    match (outcome, status) {
        (LimitedRead::Complete(bytes), Some(status)) if status.success() => {
            ReaderOutcome::Data(bytes)
        }
        _ => ReaderOutcome::Empty,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn tool_command(env: &ClipboardEnv, program: &str, args: &[&str]) -> Command {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(path) = &env.path {
        command.env("PATH", path);
    }
    command
}

/// The reference's magic-byte checks per advertised format — a tool
/// answering with the wrong shape never reaches `terminal.put`, and an
/// oversized stream only counts as too large when its capped prefix
/// still matches.
pub(crate) fn bytes_match_image_signature(extension: &str, bytes: &[u8]) -> bool {
    match extension {
        "png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "jpg" => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && bytes[8..12] == *b"WEBP",
        "bmp" => {
            if bytes.len() < 26 || !bytes.starts_with(b"BM") {
                return false;
            }
            let offset = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]) as usize;
            (26..=bytes.len()).contains(&offset)
        }
        _ => false,
    }
}

#[cfg(target_os = "linux")]
pub(crate) mod linux;

#[cfg(target_os = "linux")]
fn read_linux_clipboard_image(env: &ClipboardEnv, timeout: Duration) -> ClipboardRead {
    linux::read(env, timeout)
}
#[cfg(target_os = "linux")]
use linux::running_inside_wsl;

/// The reference's `osascript` PNG read: the clipboard is rendered into
/// a uniquely-named temp file, which is then read back bounded.
#[cfg(target_os = "macos")]
fn read_macos_clipboard_image(env: &ClipboardEnv, timeout: Duration) -> ClipboardRead {
    let path = std::env::temp_dir().join(format!(
        "hc-tui-clipboard-image-{}-{}.png",
        std::process::id(),
        timestamp_nanos()
    ));
    let script = format!(
        "set png_data to (the clipboard as \u{ab}class PNGf\u{bb})\nset fp to open for access POSIX file \"{}\" with write permission\nwrite png_data to fp\nclose access fp",
        path.display()
    );
    let command = tool_command(env, "osascript", &["-e", &script]);
    let run = read_tool(command, timeout);
    let image = match run {
        ReaderOutcome::Missing => {
            return ClipboardRead::Unavailable("Osascript could not run");
        }
        ReaderOutcome::TimedOut => {
            return ClipboardRead::Unavailable("The clipboard owner stopped answering");
        }
        // osascript emits no stdout; anything else is still only the
        // temp-file payload below.
        ReaderOutcome::Empty => match std::fs::File::open(&path)
            .ok()
            .and_then(|file| read_limited(file, MAX_CLIPBOARD_IMAGE_BYTES).ok())
        {
            Some(LimitedRead::Complete(bytes)) if bytes_match_image_signature("png", &bytes) => {
                ClipboardRead::Image(ClipboardImage {
                    bytes,
                    extension: "png",
                })
            }
            Some(LimitedRead::Oversized(prefix)) if bytes_match_image_signature("png", &prefix) => {
                ClipboardRead::TooLarge
            }
            _ => ClipboardRead::None,
        },
        _ => ClipboardRead::None,
    };
    let _ = std::fs::remove_file(&path);
    image
}

#[cfg(target_os = "macos")]
fn timestamp_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "clipboard_tests.rs"]
mod tests;

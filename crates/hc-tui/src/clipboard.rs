// Derived from herdr src/client/clipboard_images.rs and
// src/platform/{mod,linux,macos}.rs (Apache-2.0).
//! Local clipboard-image reads and dropped-image-path parsing for the
//! remote image-paste bridge (R9). Unlike the reference's unbounded
//! reads, every spawned tool here is killed after about two seconds:
//! a hung clipboard owner must not stall input forwarding, and a
//! missing tool simply counts as "no image".

use std::ffi::OsString;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// `terminal.put` accepts at most 8 MB of raw data — the bridge bound.
pub(crate) const MAX_CLIPBOARD_IMAGE_BYTES: usize = 8 * 1024 * 1024;

/// About how long one clipboard read may run before its owner is
/// declared hung. Kept a named constant so tests can shrink it.
const READER_TIMEOUT: Duration = Duration::from_secs(2);

/// One image read off the local clipboard or a dropped file.
#[derive(Debug)]
pub(crate) struct ClipboardImage {
    pub bytes: Vec<u8>,
    pub extension: &'static str,
}

/// The reader-selection facts `read_clipboard_image` needs; tests inject
/// fakes so they never touch a real clipboard.
#[derive(Debug, Default)]
pub(crate) struct ClipboardEnv {
    /// Inside WSL: `powershell.exe` answers the Windows clipboard first.
    pub wsl: bool,
    /// `WAYLAND_DISPLAY` is set: `wl-paste` may answer.
    pub wayland: bool,
    /// `DISPLAY` is set: `xclip` may answer.
    pub display: bool,
    /// PATH the spawned tools resolve against. `None` inherits the
    /// process's own; tests pass a directory of stubs.
    pub path: Option<OsString>,
}

#[cfg(target_os = "linux")]
impl ClipboardEnv {
    fn current() -> Self {
        Self {
            wsl: running_inside_wsl(),
            wayland: std::env::var_os("WAYLAND_DISPLAY").is_some(),
            display: std::env::var_os("DISPLAY").is_some(),
            path: None,
        }
    }
}

/// The image on the local clipboard, if one is offered in a supported
/// format. A missing tool, an empty clipboard, an oversized payload and
/// a timed-out owner all read as "no image".
pub(crate) fn read_clipboard_image() -> Option<ClipboardImage> {
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
        None
    }
}

/// The bounded-read outcome of one spawned tool.
pub(crate) enum LimitedRead {
    Empty,
    Complete(Vec<u8>),
    Oversized,
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
            Ok(_) => Ok(LimitedRead::Oversized),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => Err(e),
        };
    }
}

/// What one bounded tool run produced. `TimedOut` aborts the whole
/// clipboard read: a hung owner stalls every later read the same way.
enum ReaderOutcome {
    Data(Vec<u8>),
    Empty,
    TimedOut,
}

/// Spawn a clipboard tool and drain its stdout, bounded in bytes and
/// time. A missing tool, a nonzero exit and an oversized or empty
/// payload are `Empty`; a deadline kill is `TimedOut`.
fn read_tool(mut command: Command, timeout: Duration) -> ReaderOutcome {
    let mut child = match command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return ReaderOutcome::Empty,
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
    if matches!(outcome, LimitedRead::Oversized) {
        let _ = child.kill();
        let _ = child.wait();
        let _ = drain.join();
        return ReaderOutcome::Empty;
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

#[cfg(unix)]
fn tool_command(env: &ClipboardEnv, program: &str, args: &[&str]) -> Command {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(path) = &env.path {
        command.env("PATH", path);
    }
    command
}

/// The reference's magic-byte checks per advertised format — a tool
/// answering with the wrong shape never reaches `terminal.put`.
fn bytes_match_image_signature(extension: &str, bytes: &[u8]) -> bool {
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
mod linux {
    use super::*;
    use std::path::Path;

    const MIMES: [(&str, &str); 6] = [
        ("image/png", "png"),
        ("image/jpeg", "jpg"),
        ("image/jpg", "jpg"),
        ("image/gif", "gif"),
        ("image/webp", "webp"),
        ("image/bmp", "bmp"),
    ];

    /// The reference's Windows-side PowerShell clipboard read.
    const WSL_CLIPBOARD_SCRIPT: &str = "$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; $image=[System.Windows.Forms.Clipboard]::GetImage(); if ($null -eq $image) { exit 1 }; $stream=[System.IO.MemoryStream]::new(); try { $image.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png); [Console]::OpenStandardOutput().Write($stream.GetBuffer(), 0, [int]$stream.Length) } finally { $stream.Dispose(); $image.Dispose() }";

    const WSL_MARKER_ENV_VARS: &[&str] = &["WSL_DISTRO_NAME", "WSL_INTEROP"];

    pub(super) fn running_inside_wsl() -> bool {
        static WSL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *WSL.get_or_init(|| {
            proc_file_indicates_wsl("/proc/sys/kernel/osrelease")
                || proc_file_indicates_wsl("/proc/version")
                || WSL_MARKER_ENV_VARS
                    .iter()
                    .any(|key| std::env::var_os(key).is_some())
                || Path::new("/run/WSL").exists()
        })
    }

    fn proc_file_indicates_wsl(path: &str) -> bool {
        std::fs::read_to_string(path)
            .map(|text| {
                let text = text.to_ascii_lowercase();
                text.contains("microsoft") || text.contains("wsl")
            })
            .unwrap_or(false)
    }

    /// WSL → `wl-paste` (Wayland) → `xclip` (X11), in the reference's
    /// MIME order. A `TimedOut` read stops the whole attempt.
    pub(super) fn read(env: &ClipboardEnv, timeout: Duration) -> Option<ClipboardImage> {
        if env.wsl {
            let args = [
                "-NoProfile",
                "-NonInteractive",
                "-STA",
                "-Command",
                WSL_CLIPBOARD_SCRIPT,
            ];
            match read_tool(tool_command(env, "powershell.exe", &args), timeout) {
                ReaderOutcome::Data(bytes) if bytes_match_image_signature("png", &bytes) => {
                    return Some(ClipboardImage {
                        bytes,
                        extension: "png",
                    });
                }
                ReaderOutcome::TimedOut => return None,
                _ => {}
            }
        }
        for (mime, extension) in MIMES {
            for (available, program, args) in [
                (env.wayland, "wl-paste", vec!["--type", mime]),
                (
                    env.display,
                    "xclip",
                    vec!["-selection", "clipboard", "-t", mime, "-o"],
                ),
            ] {
                if !available {
                    continue;
                }
                match read_tool(tool_command(env, program, &args), timeout) {
                    ReaderOutcome::Data(bytes)
                        if bytes_match_image_signature(extension, &bytes) =>
                    {
                        return Some(ClipboardImage { bytes, extension });
                    }
                    ReaderOutcome::TimedOut => return None,
                    _ => {}
                }
            }
        }
        None
    }
}

#[cfg(target_os = "linux")]
fn read_linux_clipboard_image(env: &ClipboardEnv, timeout: Duration) -> Option<ClipboardImage> {
    linux::read(env, timeout)
}
#[cfg(target_os = "linux")]
use linux::running_inside_wsl;

/// The reference's `osascript` PNG read: the clipboard is rendered into
/// a uniquely-named temp file, which is then read back bounded.
#[cfg(target_os = "macos")]
fn read_macos_clipboard_image(env: &ClipboardEnv, timeout: Duration) -> Option<ClipboardImage> {
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
        // osascript emits no stdout; anything else is still only the
        // temp-file payload below.
        ReaderOutcome::Data(_) | ReaderOutcome::TimedOut => None,
        ReaderOutcome::Empty => std::fs::File::open(&path)
            .ok()
            .and_then(|file| read_limited(file, MAX_CLIPBOARD_IMAGE_BYTES).ok())
            .and_then(|read| match read {
                LimitedRead::Complete(bytes) if bytes_match_image_signature("png", &bytes) => {
                    Some(ClipboardImage {
                        bytes,
                        extension: "png",
                    })
                }
                _ => None,
            }),
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

// Derived from herdr src/platform/linux.rs and
// src/client/clipboard_images.rs (Apache-2.0).
//! The Linux clipboard-image read: WSL PowerShell first, then
//! `wl-paste` under Wayland and `xclip` under X11, in the reference's
//! image-MIME order, then a file-list pass that resolves a copied
//! image file on disk.

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

/// Clipboard offers that name a local image file instead of carrying
/// one: a file manager's copy (`x-special/gnome-copied-files`,
/// `text/uri-list`) or a `text/plain` holding an absolute path.
const FILE_MIMES: [&str; 3] = [
    "x-special/gnome-copied-files",
    "text/uri-list",
    "text/plain",
];

/// The notice reasons `Unavailable` can carry.
const NO_DISPLAY: &str =
    "No clipboard display is reachable (WAYLAND_DISPLAY and DISPLAY are unset)";
const NO_TOOL: &str = "No clipboard tool answered (Wayland needs wl-paste, X11 needs xclip)";
const TIMED_OUT: &str = "The clipboard owner stopped answering";

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

/// Whether any tool was tried and whether one actually ran — the
/// difference between "no image on the clipboard" and "nothing
/// could even read the clipboard". `hung` names the tools that hit
/// the deadline: a hung owner stalls every later read the same way,
/// so they are skipped and the wait stays bounded.
#[derive(Default)]
struct Attempts {
    any: bool,
    answered: bool,
    timed_out: bool,
    hung: Vec<&'static str>,
}

impl Attempts {
    /// Whether this tool may still be spawned: a tool that already
    /// hit the deadline is not asked again.
    fn may_run(&self, program: &'static str) -> bool {
        !self.hung.contains(&program)
    }

    fn outcome(&self) -> ClipboardRead {
        if self.timed_out {
            // A reachable display went unread; reporting "no image"
            // would be a guess. This also covers every tool hanging.
            return ClipboardRead::Unavailable(TIMED_OUT);
        }
        match (self.any, self.answered) {
            (false, _) => ClipboardRead::Unavailable(NO_DISPLAY),
            (true, false) => ClipboardRead::Unavailable(NO_TOOL),
            _ => ClipboardRead::None,
        }
    }
}

/// One bounded tool run counted for the attempts ledger: a spawn
/// failure tries but does not answer; a timeout marks the tool hung
/// but leaves the next tool its own turn.
fn attempt_tool(
    attempts: &mut Attempts,
    program: &'static str,
    command: Command,
    timeout: Duration,
) -> ReaderOutcome {
    attempts.any = true;
    let outcome = read_tool(command, timeout);
    match outcome {
        ReaderOutcome::Missing => {}
        ReaderOutcome::TimedOut => {
            attempts.answered = true;
            attempts.timed_out = true;
            attempts.hung.push(program);
        }
        _ => attempts.answered = true,
    }
    outcome
}

/// The per-mime tool pair: `wl-paste` when a Wayland display is
/// reachable, `xclip` under X11.
fn display_tools<'a>(env: &ClipboardEnv, mime: &'a str) -> [(bool, &'static str, Vec<&'a str>); 2] {
    [
        (env.wayland, "wl-paste", vec!["--type", mime]),
        (
            env.display,
            "xclip",
            vec!["-selection", "clipboard", "-t", mime, "-o"],
        ),
    ]
}

/// WSL → `wl-paste` (Wayland) → `xclip` (X11), in the reference's
/// MIME order, then a file-list pass that resolves a copied image
/// file on disk. A `TimedOut` run marks its tool hung — its later
/// calls are skipped so the wait stays bounded — but the next tool
/// still runs: a dead Wayland offer cannot hide an image sitting on
/// the X11 side. An oversized stream stops the read when its prefix
/// still matches the advertised image signature — that is a real
/// image, just too big; an oversized stream with a foreign prefix
/// stays "no image". When nothing could even try, or a display went
/// unread past its deadline, the read is `Unavailable`, not `None`.
pub(crate) fn read(env: &ClipboardEnv, timeout: Duration) -> ClipboardRead {
    let mut attempts = Attempts::default();
    if env.wsl {
        let args = [
            "-NoProfile",
            "-NonInteractive",
            "-STA",
            "-Command",
            WSL_CLIPBOARD_SCRIPT,
        ];
        match attempt_tool(
            &mut attempts,
            "powershell.exe",
            tool_command(env, "powershell.exe", &args),
            timeout,
        ) {
            ReaderOutcome::Data(bytes) if bytes_match_image_signature("png", &bytes) => {
                return ClipboardRead::Image(ClipboardImage {
                    bytes,
                    extension: "png",
                });
            }
            ReaderOutcome::Oversized(prefix) if bytes_match_image_signature("png", &prefix) => {
                return ClipboardRead::TooLarge;
            }
            _ => {}
        }
    }
    for (mime, extension) in MIMES {
        for (available, program, args) in display_tools(env, mime) {
            if !available || !attempts.may_run(program) {
                continue;
            }
            match attempt_tool(
                &mut attempts,
                program,
                tool_command(env, program, &args),
                timeout,
            ) {
                ReaderOutcome::Data(bytes) if bytes_match_image_signature(extension, &bytes) => {
                    return ClipboardRead::Image(ClipboardImage { bytes, extension });
                }
                ReaderOutcome::Oversized(prefix)
                    if bytes_match_image_signature(extension, &prefix) =>
                {
                    return ClipboardRead::TooLarge;
                }
                _ => {}
            }
        }
    }
    // A file copy offers no image bytes — resolve the local file it
    // names and read it back bounded, like a drop.
    for mime in FILE_MIMES {
        for (available, program, args) in display_tools(env, mime) {
            if !available || !attempts.may_run(program) {
                continue;
            }
            if let ReaderOutcome::Data(bytes) = attempt_tool(
                &mut attempts,
                program,
                tool_command(env, program, &args),
                timeout,
            ) {
                let text = String::from_utf8_lossy(&bytes);
                for (path, extension) in crate::clipdrop::image_paths_from_clipboard_text(&text) {
                    match crate::clipdrop::read_image_file(&path, extension) {
                        ClipboardRead::None => {}
                        hit => return hit,
                    }
                }
            }
        }
    }
    attempts.outcome()
}

//! Clipboard-reader unit tests. Every tool is a PATH stub; no test
//! ever touches a real clipboard.
use super::*;
use crate::clipdrop::{image_path_from_paste, read_image_file};
use std::path::{Path, PathBuf};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR";
const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0, 0, 0];
const GIF87: &[u8] = b"GIF87a\x01\x00\x01\x00";
const GIF89: &[u8] = b"GIF89a\x01\x00\x01\x00";
const WEBP: &[u8] = b"RIFF\x10\x00\x00\x00WEBPVP8 ";
const NOT_AN_IMAGE: &[u8] = b"plain text, not an image";

fn bmp() -> Vec<u8> {
    let mut bytes = vec![0u8; 30];
    bytes[0] = b'B';
    bytes[1] = b'M';
    bytes[10..14].copy_from_slice(&26u32.to_le_bytes());
    bytes
}

#[test]
fn signatures_validate_every_supported_format() {
    assert!(bytes_match_image_signature("png", PNG));
    assert!(bytes_match_image_signature("jpg", JPEG));
    assert!(bytes_match_image_signature("gif", GIF87));
    assert!(bytes_match_image_signature("gif", GIF89));
    assert!(bytes_match_image_signature("webp", WEBP));
    assert!(bytes_match_image_signature("bmp", &bmp()));
    assert!(!bytes_match_image_signature("png", &PNG[..4]));
    assert!(!bytes_match_image_signature("jpg", &JPEG[..2]));
    assert!(!bytes_match_image_signature("gif", &GIF87[..4]));
    assert!(!bytes_match_image_signature(
        "webp",
        b"RIFF\x10\x00\x00\x00WAVE"
    ));
    assert!(!bytes_match_image_signature("bmp", &bmp()[..25]));
    let mut bad_bmp = bmp();
    bad_bmp[10..14].copy_from_slice(&9999u32.to_le_bytes());
    assert!(!bytes_match_image_signature("bmp", &bad_bmp));
    assert!(!bytes_match_image_signature("png", NOT_AN_IMAGE));
}

/// A unique scratch dir of fake tools: `name` -> shell body. The PATH
/// override makes them the only tools the readers can find.
struct StubDir(PathBuf);

impl StubDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "hc-tui-clipboard-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn stub(&self, name: &str, body: &str) {
        let path = self.0.join(name);
        std::fs::write(
            &path,
            format!("#!/bin/sh\n[ -n \"$STUB_PROBE\" ] && exit 0\n{body}\n"),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        wait_executable(&path);
    }
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
    fn env(&self) -> ClipboardEnv {
        let path = format!("{}:/usr/bin:/bin", self.0.display());
        ClipboardEnv {
            path: Some(path.into()),
            ..ClipboardEnv::default()
        }
    }
}

impl Drop for StubDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A fork in another test thread can briefly pin the just-written
/// script's open writer fd, and a racing `exec` then fails with ETXTBSY
/// (observed as a flaky `wsl_powershell_is_tried_first` failure). The
/// sentinel probe runs the script as a no-op until that transient
/// clears; production reads get no retry.
fn wait_executable(path: &Path) {
    for _ in 0..200 {
        match std::process::Command::new(path)
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

/// The reader-path image expectation — everything else fails the test.
#[cfg(target_os = "linux")]
fn linux_image(env: &ClipboardEnv) -> ClipboardImage {
    match linux::read(env, Duration::from_secs(5)) {
        ClipboardRead::Image(image) => image,
        other => panic!("expected an image, got {other:?}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn wl_paste_serves_the_first_matching_mime() {
    let dir = StubDir::new();
    let payload = dir.write("payload", JPEG);
    dir.stub(
        "wl-paste",
        &format!(
            "case \"$2\" in image/jpeg) cat \"{}\" ;; *) exit 1 ;; esac",
            payload.display()
        ),
    );
    let env = ClipboardEnv {
        wayland: true,
        ..dir.env()
    };
    let image = linux_image(&env);
    assert_eq!(image.extension, "jpg");
    assert_eq!(image.bytes, JPEG);
}

#[cfg(target_os = "linux")]
#[test]
fn xclip_answers_when_wayland_is_absent() {
    let dir = StubDir::new();
    let payload = dir.write("payload", GIF89);
    dir.stub(
        "xclip",
        &format!(
            "case \"$4\" in image/gif) cat \"{}\" ;; *) exit 1 ;; esac",
            payload.display()
        ),
    );
    let env = ClipboardEnv {
        display: true,
        ..dir.env()
    };
    let image = linux_image(&env);
    assert_eq!(image.extension, "gif");
    assert_eq!(image.bytes, GIF89);
}

#[cfg(target_os = "linux")]
#[test]
fn wsl_powershell_is_tried_first() {
    let dir = StubDir::new();
    let payload = dir.write("payload", PNG);
    dir.stub("powershell.exe", &format!("cat \"{}\"", payload.display()));
    let env = ClipboardEnv {
        wsl: true,
        ..dir.env()
    };
    let image = linux_image(&env);
    assert_eq!(image.extension, "png");
    assert_eq!(image.bytes, PNG);
}

#[cfg(target_os = "linux")]
#[test]
fn missing_tool_is_no_image() {
    let dir = StubDir::new();
    let env = ClipboardEnv {
        wayland: true,
        display: true,
        ..dir.env()
    };
    assert!(matches!(
        linux::read(&env, Duration::from_secs(1)),
        ClipboardRead::None
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn hung_tool_is_killed_and_counts_as_no_image() {
    let dir = StubDir::new();
    dir.stub("wl-paste", "sleep 30");
    let env = ClipboardEnv {
        wayland: true,
        ..dir.env()
    };
    let start = Instant::now();
    assert!(matches!(
        linux::read(&env, Duration::from_millis(150)),
        ClipboardRead::None
    ));
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[cfg(target_os = "linux")]
#[test]
fn invalid_signature_is_skipped() {
    let dir = StubDir::new();
    dir.stub("wl-paste", "echo not-an-image");
    let env = ClipboardEnv {
        wayland: true,
        ..dir.env()
    };
    assert!(matches!(
        linux::read(&env, Duration::from_secs(5)),
        ClipboardRead::None
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn oversized_non_image_is_still_no_image() {
    let dir = StubDir::new();
    dir.stub(
        "wl-paste",
        &format!("head -c {} /dev/zero", MAX_CLIPBOARD_IMAGE_BYTES + 64),
    );
    let env = ClipboardEnv {
        wayland: true,
        ..dir.env()
    };
    assert!(matches!(
        linux::read(&env, Duration::from_secs(15)),
        ClipboardRead::None
    ));
}

/// A real reader run: the stub serves more than the upload bound of
/// valid-looking PNG, so the read is `TooLarge` — never silently
/// counted as "no image".
#[cfg(target_os = "linux")]
#[test]
fn oversized_valid_png_is_too_large() {
    let dir = StubDir::new();
    let mut bytes = PNG.to_vec();
    bytes.resize(MAX_CLIPBOARD_IMAGE_BYTES + 64, 0);
    let payload = dir.write("big.png", &bytes);
    dir.stub("wl-paste", &format!("cat \"{}\"", payload.display()));
    let env = ClipboardEnv {
        wayland: true,
        ..dir.env()
    };
    assert!(matches!(
        linux::read(&env, Duration::from_secs(15)),
        ClipboardRead::TooLarge
    ));
}

#[test]
fn paste_path_parses_plain_quoted_and_escaped_forms() {
    for (text, expected) in [
        ("/home/user/a.png", "/home/user/a.png"),
        ("'/home/user/a b.png'", "/home/user/a b.png"),
        ("\"/home/user/a b.png\"", "/home/user/a b.png"),
        ("/home/user/a\\ b.png", "/home/user/a b.png"),
        ("/home/user/a.png\n", "/home/user/a.png"),
        ("/tmp/x.JPEG", "/tmp/x.JPEG"),
        ("/tmp/x.webp", "/tmp/x.webp"),
        ("/tmp/x.BMP", "/tmp/x.BMP"),
    ] {
        let (path, _) = image_path_from_paste(text).unwrap_or_else(|| panic!("{text:?}"));
        assert_eq!(path, PathBuf::from(expected), "{text:?}");
    }
    assert_eq!(image_path_from_paste("/x.png").unwrap().1, "png");
    assert_eq!(image_path_from_paste("/x.jpg").unwrap().1, "jpg");
    assert_eq!(image_path_from_paste("/x.jpeg").unwrap().1, "jpg");
    assert_eq!(image_path_from_paste("/x.gif").unwrap().1, "gif");
}

#[test]
fn paste_path_rejects_non_images() {
    for text in [
        "relative/path.png",
        "",
        "/home/user/a.png\n/home/user/b.png",
        "/home/user/a.txt",
        "/home/user/no-extension",
        "/home/user/.hidden",
    ] {
        assert!(image_path_from_paste(text).is_none(), "{text:?}");
    }
}

#[test]
fn image_file_reads_only_regular_files_under_the_limit() {
    let dir = StubDir::new();
    let png = dir.write("a.png", PNG);
    let ClipboardRead::Image(image) = read_image_file(&png, "png") else {
        panic!("png image")
    };
    assert_eq!(image.bytes, PNG);
    assert!(
        matches!(read_image_file(&dir.0, "png"), ClipboardRead::None),
        "a directory"
    );
    assert!(matches!(
        read_image_file(&dir.0.join("gone.png"), "png"),
        ClipboardRead::None
    ));
    let empty = dir.write("empty.png", &[]);
    assert!(
        matches!(read_image_file(&empty, "png"), ClipboardRead::None),
        "empty file"
    );
    // Over the bound with a foreign prefix: still not an image.
    let junk_bytes = vec![0u8; MAX_CLIPBOARD_IMAGE_BYTES + 1];
    let junk = dir.write("junk.png", &junk_bytes);
    assert!(
        matches!(read_image_file(&junk, "png"), ClipboardRead::None),
        "oversized non-image"
    );
    // Over the bound with a matching signature prefix: too large.
    let mut big_bytes = PNG.to_vec();
    big_bytes.resize(MAX_CLIPBOARD_IMAGE_BYTES + 1, 0);
    let big = dir.write("big.png", &big_bytes);
    assert!(
        matches!(read_image_file(&big, "png"), ClipboardRead::TooLarge),
        "oversized image"
    );
}

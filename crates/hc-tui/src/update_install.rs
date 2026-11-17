//! Where an in-client upgrade installs: the directory of the running
//! binary (spec 5.1), whether that directory accepts writes, and the
//! shell quoting every command line built from it needs.

use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The running binary's canonical path: symlinks resolve, and a binary
/// replaced while running (`" (deleted)"`) falls back to its named path.
pub(crate) fn canonical_exe(exe: &Path) -> PathBuf {
    std::fs::canonicalize(exe).unwrap_or_else(|_| {
        let text = exe.to_string_lossy();
        PathBuf::from(text.strip_suffix(" (deleted)").unwrap_or(&text))
    })
}

/// The directory the running `hc-tui` lives in — the one the installer
/// must write, so the binary that is running is the one replaced.
/// Symlinks resolve; on Linux a binary replaced while running shows a
/// `" (deleted)"` suffix, which the fallback strips.
pub(crate) fn install_dir_for(exe: &Path) -> Option<PathBuf> {
    canonical_exe(exe).parent().map(Path::to_path_buf)
}

/// The directory of the running binary, or `None` when it is unknown.
pub(crate) fn running_install_dir(exe: Option<&Path>) -> Option<PathBuf> {
    let exe = match exe {
        Some(exe) => exe.to_path_buf(),
        None => std::env::current_exe().ok()?,
    };
    install_dir_for(&exe)
}

/// Where `hc-tui --upgrade` installs: the running binary's directory,
/// unless `HC_TUI_INSTALL_DIR` already names one (an empty value counts as
/// unset). `None` leaves the installer's own default in charge.
pub(crate) fn upgrade_install_dir(
    configured: Option<&OsStr>,
    exe: Option<&Path>,
) -> Option<PathBuf> {
    match configured.filter(|dir| !dir.is_empty()) {
        Some(_) => None,
        None => running_install_dir(exe),
    }
}

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Whether `dir` accepts a new file: a `create_new` probe that is
/// removed at once, as herdr's writability check does. Never touches
/// an existing file.
pub(crate) fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(
        ".hc-tui-write-probe.{}.{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    match OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// `$HOME/.local/bin` — the installer's default, which the manual
/// command can omit `HC_TUI_INSTALL_DIR` for.
pub(crate) fn default_install_dir(home: Option<&OsStr>) -> Option<PathBuf> {
    home.filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(".local/bin"))
}

/// A POSIX single-quoted word: bare when every byte is safe, otherwise
/// wrapped in `'…'` with each embedded `'` written as `'\''`.
pub(crate) fn shell_quote(text: &str) -> String {
    let safe = !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_.-".contains(&b));
    if safe {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

/// The manual upgrade command for `dir`: plain `hc-tui --upgrade` when
/// `dir` is the installer's default, else with `HC_TUI_INSTALL_DIR`.
pub(crate) fn manual_command(dir: &Path, default_dir: Option<&Path>) -> String {
    if default_dir == Some(dir) {
        "hc-tui --upgrade".to_owned()
    } else {
        format!(
            "HC_TUI_INSTALL_DIR={} hc-tui --upgrade",
            shell_quote(&dir.to_string_lossy())
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory that removes itself.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!(
                "hctui-install-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_writable_directory_passes_the_probe_and_leaves_nothing() {
        let scratch = Scratch::new("writable");
        assert!(dir_writable(&scratch.0));
        let leftovers = std::fs::read_dir(&scratch.0).unwrap().count();
        assert_eq!(leftovers, 0, "the probe removes its own file");
    }

    #[test]
    fn a_missing_directory_is_not_writable() {
        let scratch = Scratch::new("missing");
        assert!(!dir_writable(&scratch.0.join("no-such-dir")));
    }

    #[test]
    fn the_binary_directory_is_the_install_directory() {
        assert_eq!(
            install_dir_for(Path::new("/home/user/bin/hc-tui")),
            Some(PathBuf::from("/home/user/bin"))
        );
        assert_eq!(
            install_dir_for(Path::new("/opt/hc/hc-tui (deleted)")),
            Some(PathBuf::from("/opt/hc"))
        );
    }

    #[test]
    fn a_binary_in_a_home_bin_names_that_directory_for_the_installer() {
        let dir = Path::new("/home/user/bin");
        assert_eq!(
            manual_command(dir, Some(Path::new("/home/user/.local/bin"))),
            "HC_TUI_INSTALL_DIR=/home/user/bin hc-tui --upgrade"
        );
        assert_eq!(
            manual_command(
                Path::new("/home/user/.local/bin"),
                default_install_dir(Some(OsStr::new("/home/user"))).as_deref()
            ),
            "hc-tui --upgrade",
            "the default directory needs no override"
        );
    }

    #[test]
    fn quoting_keeps_safe_paths_bare_and_escapes_single_quotes() {
        assert_eq!(
            shell_quote("/home/user/.local/bin"),
            "/home/user/.local/bin"
        );
        assert_eq!(shell_quote("/srv/my bin"), "'/srv/my bin'");
        assert_eq!(
            shell_quote("/home/user/it's here"),
            "'/home/user/it'\\''s here'"
        );
        assert_eq!(shell_quote(""), "''");
    }
}

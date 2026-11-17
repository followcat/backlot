//! Runner presence probes for the `studio new` dry run (D19): a
//! runner the draft names must have an adapter (`supported`) and a
//! launch file this shell can run (`installed`) — `fake`'s script in
//! `tools/`, every other runner's program on PATH.

use std::path::Path;

use super::tools_dir;

/// Every runner name `plan` has an adapter for — the dry-run's
/// "runner supported" check without building a launch.
pub fn supported(runner: &str) -> bool {
    matches!(
        runner,
        "fake" | "claude" | "codex" | "devin" | "opencode" | "cursor" | "cmd" | "cline"
    )
}

/// The program the runner's argv leads with — `None` for `fake`,
/// whose script ships in `tools/` instead of on PATH.
fn launch_program(runner: &str) -> Option<&'static str> {
    match runner {
        "claude" => Some("claude"),
        "codex" => Some("codex"),
        "devin" => Some("devin"),
        "opencode" => Some("opencode"),
        "cursor" => Some("cursor-agent"),
        "cmd" => Some("cmd"),
        "cline" => Some("cline"),
        "fake" => None,
        _ => None,
    }
}

/// An executable regular file — the executable bit on unix, a plain
/// file elsewhere.
fn executable(f: &Path) -> bool {
    if !f.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(f)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// `name` resolves to an executable file on `path` (the PATH value).
fn on_path(name: &str, path: Option<&str>) -> bool {
    let Some(path) = path else {
        return false;
    };
    path.split(':')
        .filter(|dir| !dir.is_empty())
        .any(|dir| executable(&Path::new(dir).join(name)))
}

/// The runner's launch file exists: `fake`'s tools script, every
/// other runner's program on PATH. The dry-run's "runner installed"
/// check.
pub fn installed(runner: &str, path: Option<&str>) -> bool {
    match launch_program(runner) {
        Some(prog) => on_path(prog, path),
        None => runner == "fake" && tools_dir().join("fake-runner.sh").is_file(),
    }
}

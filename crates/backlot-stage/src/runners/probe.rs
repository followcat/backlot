//! Runner presence probes for the `studio new` dry run (D19): a
//! runner the draft names must have an adapter (`supported`) and a
//! launch file this shell can run (`installed`) — `fake`'s script in
//! `tools/`, every other runner's program on PATH.

use std::path::{Path, PathBuf};

use super::tools_dir;

/// The env var a rig exports so an installed `backlot` launches the
/// fake runner from the rig's own checkout.
pub(crate) const FAKE_RUNNER_ENV: &str = "BACKLOT_FAKE_RUNNER";

/// The fake runner's script: a nonempty `BACKLOT_FAKE_RUNNER` wins,
/// else the shipped `tools/fake-runner.sh`. The compile-time fallback
/// vanishes when the build tree is removed, which is why the rig
/// exports its own checkout's script.
pub(crate) fn fake_runner_path(override_path: Option<&str>) -> PathBuf {
    match override_path.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => tools_dir().join("fake-runner.sh"),
    }
}

/// The nonempty `BACKLOT_FAKE_RUNNER` a launch sees: a stage's own
/// environment first, then the process's, empty values skipped.
pub(crate) fn fake_runner_override(stage_value: Option<&str>) -> Option<String> {
    fn nonempty(v: String) -> Option<String> {
        Some(v).filter(|v| !v.trim().is_empty())
    }
    stage_value
        .map(str::to_string)
        .and_then(nonempty)
        .or_else(|| {
            std::env::var_os(FAKE_RUNNER_ENV)
                .map(|v| v.to_string_lossy().into_owned())
                .and_then(nonempty)
        })
}

/// Every runner name `plan` has an adapter for — the dry-run's
/// "runner supported" check without building a launch.
pub fn supported(runner: &str) -> bool {
    matches!(
        runner,
        "fake" | "claude" | "codex" | "devin" | "opencode" | "cursor" | "cmd" | "cline" | "agy"
    )
}

/// The program the runner's argv leads with — `None` for `fake`,
/// whose script ships in `tools/` instead of on PATH.
pub(crate) fn launch_program(runner: &str) -> Option<&'static str> {
    match runner {
        "claude" => Some("claude"),
        "codex" => Some("codex"),
        "devin" => Some("devin"),
        "opencode" => Some("opencode"),
        "cursor" => Some("cursor-agent"),
        "cmd" => Some("cmd"),
        "cline" => Some("cline"),
        "agy" => Some("agy"),
        "fake" => None,
        _ => None,
    }
}

/// An executable regular file — the executable bit on unix, a plain
/// file elsewhere.
pub(crate) fn executable(f: &Path) -> bool {
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

/// `name`'s first executable file on `path` (the PATH value) — the
/// directory order kept, like the shell's own search.
pub(crate) fn find_on_path(name: &str, path: Option<&str>) -> Option<PathBuf> {
    path?
        .split(':')
        .filter(|dir| !dir.is_empty())
        .map(|dir| Path::new(dir).join(name))
        .find(|f| executable(f))
}

/// `name` resolves to an executable file on `path` (the PATH value).
fn on_path(name: &str, path: Option<&str>) -> bool {
    find_on_path(name, path).is_some()
}

/// The runner's launch file exists: `fake`'s `BACKLOT_FAKE_RUNNER`
/// script (or its `tools/` script when unset), every other runner's
/// program on PATH. The dry-run's "runner installed" check.
pub fn installed(runner: &str, path: Option<&str>) -> bool {
    match launch_program(runner) {
        Some(prog) => on_path(prog, path),
        None => runner == "fake" && fake_runner_path(probe_override().as_deref()).is_file(),
    }
}

/// The process environment's `BACKLOT_FAKE_RUNNER`, since `installed`
/// takes no injected environment.
fn probe_override() -> Option<String> {
    fake_runner_override(None)
}

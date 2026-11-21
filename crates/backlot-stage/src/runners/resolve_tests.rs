//! `resolve` — the launch-side verdict the picker's "detected"
//! list, `studio new --dry-run` and the role's launch all share:
//! a configured `[runner_env]` PATH is the whole launch PATH (the
//! launch env wins, both directions), and without one detection's
//! order — inherited PATH, the login shell's, the well-known
//! dirs — stands, so an offered runner is a launchable runner.

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

use crate::cli::Env;
use crate::runners::detect::{self, Scope, Source};
use crate::runners::resolve;
use crate::test_support::TestDir;

/// One scratch root per test.
fn dir(name: &str) -> TestDir {
    TestDir::new(&format!("backlot-resolve-test-{name}"))
}

/// An executable shell script `<dir>/<name>` running `body`.
fn exe(dir: &Path, name: &str, body: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

/// A fake login shell that rebuilds PATH around `login_bin` — the
/// detect tests' shape.
fn fake_shell(dir: &Path, login_bin: &Path) -> PathBuf {
    exe(
        dir,
        "fake-shell",
        &format!(
            "echo profile-noise\nexport PATH=\"{}\"\nshift\nexec /bin/sh -c \"$1\"",
            login_bin.display()
        ),
    )
}

fn scope(env: &Env) -> Scope<'_> {
    detect::scope(env)
}

fn env(vars: &[(&str, &str)]) -> Env {
    Env {
        vars: vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>(),
    }
}

/// A configured `[runner_env]` PATH is the whole launch PATH: the
/// binary sits on the inherited PATH but the pin does not reach
/// it, so the resolve refuses — the user's own PATH wins — and
/// the refusal names the PATH that was searched.
#[test]
fn a_configured_path_is_the_whole_launch_env() {
    let root = dir("configured");
    let bin = root.join("bin");
    exe(&bin, "claude", "echo 2.1.330");
    let pinned = root.join("pinned");
    std::fs::create_dir_all(&pinned).unwrap();
    let env = env(&[("PATH", &bin.to_string_lossy())]);
    let config = format!("[runner_env.claude]\nPATH = \"{}\"\n", pinned.display());
    let err = resolve::launch_binary("claude", &config, &scope(&env)).unwrap_err();
    assert!(err.contains("not installed"), "{err}");
    assert!(
        err.contains(&pinned.display().to_string()),
        "the searched PATH is the configured one: {err}"
    );
}

/// A binary only the login shell's PATH reaches still resolves —
/// it is launchable through the `[runner_env]` write
/// `ensure_runner_paths` records at `studio new`.
#[test]
fn a_login_shell_hit_resolves() {
    let root = dir("login");
    let login = root.join("login-bin");
    let claude = exe(&login, "claude", "echo 0.55.0");
    let shell = fake_shell(&root, &login);
    let env = env(&[
        ("PATH", "/nonexistent-dir"),
        ("SHELL", &shell.to_string_lossy()),
        ("HOME", &root.join("home").to_string_lossy()),
    ]);
    let found = resolve::launch_binary("claude", "", &scope(&env)).unwrap();
    assert_eq!(found.path, claude);
    assert_eq!(found.source, Source::Login);
    assert!(found.needs_path());
}

/// A miss names the launch PATH that was searched — and that the
/// wider search ran too — so the refusal is actionable.
#[test]
fn a_miss_names_the_searched_path() {
    let root = dir("miss");
    let env = env(&[
        ("PATH", "/nonexistent-dir"),
        ("HOME", &root.join("home").to_string_lossy()),
    ]);
    let err = resolve::launch_binary("cline", "", &scope(&env)).unwrap_err();
    assert!(err.contains("searched PATH=/nonexistent-dir"), "{err}");
    assert!(
        err.contains("login shell") && err.contains("known install dirs"),
        "the wider search is named too: {err}"
    );
}

/// `fake` never waits on PATH at all — its `tools/` script is the
/// launch file wherever the repo is.
#[test]
fn fake_resolves_through_the_tools_script() {
    let env = env(&[("PATH", "/nonexistent-dir")]);
    assert!(resolve::launch_binary("fake", "", &scope(&env)).is_ok());
}

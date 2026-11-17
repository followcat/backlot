//! `detect` tests (U5): fake directory trees under a temporary
//! HOME, fake executables that print a version, a fake `$SHELL`
//! that exports a login PATH — no real agent is ever needed. The
//! `run` seam stays the real `run_timed`: a shell script is a real
//! child, so a failing or hanging `--version` is exercised for
//! real too.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::cli::Env;
use crate::runners::detect::{self, Scope, Source};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// One scratch root per test.
fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "backlot-detect-test-{}-{}-{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
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

/// A fake login shell: prints start-up noise, replaces PATH with
/// `login_bin` — like a profile rebuilding it from scratch — then
/// runs the `-lc` command with /bin/sh.
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

/// The real seams over a hand-built environment: `env` is the only
/// map the search reads, `run_timed` is the real child runner.
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

/// A binary on the inherited PATH is found first — no PATH write
/// is ever needed for it.
#[test]
fn the_inherited_path_wins_first() {
    let root = dir("inherited");
    let bin = root.join("bin");
    let claude = exe(&bin, "claude", "echo 2.1.330");
    // A login shell and a known dir both carry it too — the
    // inherited PATH still wins.
    let login = root.join("login");
    exe(&login, "claude", "echo 9.9.9");
    let shell = fake_shell(&root, &login);
    let home = root.join("home");
    exe(&home.join(".local/bin"), "claude", "echo 8.8.8");
    let env = env(&[
        ("PATH", &bin.to_string_lossy()),
        ("SHELL", &shell.to_string_lossy()),
        ("HOME", &home.to_string_lossy()),
    ]);
    let found = detect::find("claude", &scope(&env)).unwrap();
    assert_eq!(found.path, claude);
    assert_eq!(found.version.as_deref(), Some("2.1.330"));
    assert_eq!(found.source, Source::Inherited);
    assert!(!found.needs_path());
}

/// A binary only the login shell's PATH reaches: `Source::Login`,
/// "needs PATH" — its directory must reach the runner env.
#[test]
fn the_login_shell_path_is_the_second_place() {
    let root = dir("login");
    let login = root.join("login-bin");
    let codex = exe(&login, "codex", "echo 0.55.0");
    let shell = fake_shell(&root, &login);
    let home = root.join("home");
    let env = env(&[
        ("PATH", "/nonexistent-dir"),
        ("SHELL", &shell.to_string_lossy()),
        ("HOME", &home.to_string_lossy()),
    ]);
    let found = detect::find("codex", &scope(&env)).unwrap();
    assert_eq!(found.path, codex);
    assert_eq!(found.version.as_deref(), Some("0.55.0"));
    assert_eq!(found.source, Source::Login);
    assert!(found.needs_path());
}

/// Under `~/.nvm/versions/node/*/bin` the newest version's binary
/// wins, and `Source::Known` is "needs PATH" too.
#[test]
fn the_newest_nvm_version_wins() {
    let root = dir("nvm");
    let home = root.join("home");
    let node = home.join(".nvm/versions/node");
    exe(&node.join("v8.1.0/bin"), "claude", "echo 8.1.0");
    exe(&node.join("v24.19.0/bin"), "claude", "echo 24.19.0");
    // A name that parses no version sorts after every real one.
    exe(&node.join("lts/bin"), "claude", "echo 0.0.1");
    let env = env(&[
        ("PATH", "/nonexistent-dir"),
        ("HOME", &home.to_string_lossy()),
    ]);
    let found = detect::find("claude", &scope(&env)).unwrap();
    assert_eq!(
        found.path,
        node.join("v24.19.0/bin").join("claude"),
        "{}",
        found.path.display()
    );
    assert_eq!(found.version.as_deref(), Some("24.19.0"));
    assert_eq!(found.source, Source::Known);
    assert!(found.needs_path());
}

/// The first hit wins — even when its `--version` fails. A PATH
/// `claude` that cannot start is reported "not working" at its own
/// source, never skipped for a login-shell one behind it.
#[test]
fn a_binary_that_fails_version_is_not_working() {
    let root = dir("broken");
    let bin = root.join("bin");
    let claude = exe(&bin, "claude", "exit 1");
    let login = root.join("login");
    exe(&login, "claude", "echo 9.9.9");
    let shell = fake_shell(&root, &login);
    let env = env(&[
        ("PATH", &bin.to_string_lossy()),
        ("SHELL", &shell.to_string_lossy()),
        ("HOME", &root.join("home").to_string_lossy()),
    ]);
    let found = detect::find("claude", &scope(&env)).unwrap();
    assert_eq!(found.path, claude);
    assert_eq!(found.version, None);
    assert_eq!(found.source, Source::Inherited);
}

/// A `--version` that never answers is the same "not working": the
/// child is killed at the timeout, the hit still reported.
#[test]
fn a_hanging_version_probe_is_killed() {
    let root = dir("hanging");
    let bin = root.join("bin");
    let claude = exe(&bin, "claude", "sleep 10");
    let env = env(&[("PATH", &bin.to_string_lossy())]);
    let found = detect::find("claude", &scope(&env)).unwrap();
    assert_eq!(found.path, claude);
    assert_eq!(found.version, None);
}

/// Escape sequences and control characters are stripped from the
/// version line, and only its first line counts.
#[test]
fn the_version_line_is_cleaned() {
    let root = dir("clean");
    let bin = root.join("bin");
    exe(
        &bin,
        "devin",
        "printf '\\033[32m1.2.3\\033[0m\\r\\nsecond line\\n'",
    );
    let env = env(&[("PATH", &bin.to_string_lossy())]);
    let found = detect::find("devin", &scope(&env)).unwrap();
    assert_eq!(found.version.as_deref(), Some("1.2.3"));
}

/// Nothing on PATH, no `$SHELL`, no known dir: the runner is
/// simply missing — `find` answers `None`.
#[test]
fn missing_everywhere_is_none() {
    let root = dir("missing");
    let env = env(&[
        ("PATH", "/nonexistent-dir"),
        ("HOME", &root.join("home").to_string_lossy()),
    ]);
    assert!(detect::find("cline", &scope(&env)).is_none());
}

/// An unset `$SHELL` skips the login-shell step quietly — the
/// known dirs still answer.
#[test]
fn no_shell_skips_the_login_step() {
    let root = dir("noshell");
    let home = root.join("home");
    let cline = exe(&home.join(".volta/bin"), "cline", "echo 1.0.0");
    let env = env(&[
        ("PATH", "/nonexistent-dir"),
        ("HOME", &home.to_string_lossy()),
    ]);
    let found = detect::find("cline", &scope(&env)).unwrap();
    assert_eq!(found.path, cline);
    assert_eq!(found.source, Source::Known);
}

/// `backlot runners` prints every runner with its verdict — the
/// binary's path `~`-shortened when it lives under HOME, its
/// version, "needs PATH" only for a login-shell or known-dir hit.
#[test]
fn the_runners_table_shows_each_verdict() {
    let root = dir("table");
    let home = root.join("home");
    let bin = root.join("bin");
    exe(&bin, "claude", "echo 2.1.330");
    let login = root.join("login");
    let codex = exe(&login, "codex", "echo 0.55.0");
    let shell = fake_shell(&root, &login);
    let cline = exe(&home.join(".volta/bin"), "cline", "echo 1.0.0");
    let env = env(&[
        ("PATH", &bin.to_string_lossy()),
        ("SHELL", &shell.to_string_lossy()),
        ("HOME", &home.to_string_lossy()),
    ]);
    let out = detect::report(&env).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[0].split_whitespace().collect::<Vec<_>>(),
        ["runner", "status", "version", "path", "needs", "PATH"],
        "{out}"
    );
    let row = |runner: &str| {
        lines
            .iter()
            .find(|l| l.starts_with(&format!("{runner} ")))
            .copied()
            .unwrap_or_else(|| panic!("no row for {runner}:\n{out}"))
    };
    // claude on the inherited PATH: found, no PATH write.
    assert!(
        row("claude").contains("found") && row("claude").contains("2.1.330"),
        "{}",
        row("claude")
    );
    assert!(row("claude").contains(&bin.join("claude").to_string_lossy().to_string()));
    // codex only on the login shell's PATH: needs PATH yes.
    let codex_row = row("codex");
    assert!(codex_row.contains("yes"), "{codex_row}");
    assert!(codex_row.contains(&codex.to_string_lossy().to_string()));
    // cline under HOME displays `~`-shortened.
    let cline_row = row("cline");
    assert!(
        cline_row.contains("~/.volta/bin/cline"),
        "{cline_row} (real path {})",
        cline.display()
    );
    // Everyone else is missing — path and version are dashes.
    for runner in ["devin", "cursor", "opencode", "cmd"] {
        let r = row(runner);
        assert!(r.contains("missing"), "{r}");
        assert!(r.contains(" - "), "{r}");
    }
}

/// `backlot runners` parses, needs no core and no flags beyond the
/// common ones.
#[test]
fn the_command_needs_nothing_but_args() {
    let (_scratch, env) = crate::cli_testkit::scratch();
    let out = crate::cli_testkit::ok(&env, "runners");
    assert!(out.contains("runner"), "{out}");
    assert!(out.contains("claude"), "{out}");
    crate::cli_testkit::usage(&env, "runners --bogus");
}

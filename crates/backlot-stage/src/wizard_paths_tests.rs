//! The `[runner_env]` PATH write (U5/A10): a fresh core has no state
//! directory yet — `paths::initialize` runs later — so the first
//! `config.toml` a login-PATH-only runner's fixup writes must create
//! its parent directory itself rather than fail `No such file or
//! directory`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::cli::Env;
use crate::runners::detect::{Found, Source};
use crate::test_support::TestDir;
use crate::wizard_paths::ensure_runner_paths;

fn env(vars: &[(&str, &str)]) -> Env {
    Env {
        vars: vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    }
}

/// A runner detection reached only off the inherited PATH, at
/// `path` — what `needs_path()` keys on.
fn found_at(path: PathBuf) -> Found {
    Found {
        path,
        version: Some("1".into()),
        source: Source::Login,
    }
}

/// A base directory that does not exist yet — a fresh core's state
/// dir before `paths::initialize` makes it.
#[test]
fn a_missing_state_dir_is_created_for_the_first_config_write() {
    let root = TestDir::new("backlot-wizard-paths-test");
    // Deliberately not created: the whole point of A10.
    let base = root.join("state/core-fresh");
    assert!(!base.exists(), "the base starts absent");

    let bin_dir = root.join(".opencode/bin");
    let found = found_at(bin_dir.join("opencode"));
    let mut out = Vec::new();
    let mut detect = |_: &str| Some(found.clone());
    let named: BTreeSet<String> = ["opencode".to_string()].into();

    ensure_runner_paths(
        &base,
        &named,
        &env(&[("PATH", "/usr/bin:/bin")]),
        &mut detect,
        &mut out,
    )
    .expect("a missing state dir must not fail the fixup");

    let config = base.join("config.toml");
    let text = std::fs::read_to_string(&config)
        .unwrap_or_else(|e| panic!("config.toml was not written: {e}"));
    assert!(
        text.contains("[runner_env.opencode]"),
        "the table is written: {text}"
    );
    assert!(text.contains("PATH = "), "with its PATH line: {text}");
    assert!(
        text.contains(&bin_dir.to_string_lossy().into_owned()),
        "prepending the binary's own dir: {text}"
    );

    // Born `0600` like every other first write.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&config).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the new config is private");
    }
}

/// The parent directory is only created on the *create* branch: an
/// existing `config.toml` keeps its inode and mode, and a second
/// runner's fixup appends rather than replacing.
#[test]
fn an_existing_config_keeps_its_file_and_gains_the_table() {
    let root = TestDir::new("backlot-wizard-paths-test");
    let base = root.join("state/core-fresh");
    std::fs::create_dir_all(&base).unwrap();
    let config = base.join("config.toml");
    std::fs::write(&config, "# user's own notes\n").unwrap();

    let found = found_at(root.join(".nvm/bin/opencode"));
    let mut out = Vec::new();
    let mut detect = |_: &str| Some(found.clone());
    let named: BTreeSet<String> = ["opencode".to_string()].into();
    ensure_runner_paths(
        &base,
        &named,
        &env(&[("PATH", "/usr/bin")]),
        &mut detect,
        &mut out,
    )
    .unwrap();

    let text = std::fs::read_to_string(&config).unwrap();
    assert!(
        text.contains("# user's own notes"),
        "the user's file survives: {text}"
    );
    assert!(
        text.contains("[runner_env.opencode]"),
        "the table is appended: {text}"
    );
}

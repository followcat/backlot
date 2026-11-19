//! Install-source detection from temp-dir paths and an injected
//! environment. Nothing here runs an installer or reads a real mise,
//! Homebrew, Nix or cargo tree; the Nix path is only a string.

use super::*;
use crate::test_support::TestDir;
use crate::update_install::canonical_exe;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// A scratch root and the guard that removes it — canonical, so the
/// install-source prefix checks compare like paths.
struct Base {
    _dir: TestDir,
    path: PathBuf,
}

impl std::ops::Deref for Base {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

fn base(label: &str) -> Base {
    let dir = TestDir::new(&format!("hctui-source-{label}"));
    let path = std::fs::canonicalize(&dir).unwrap();
    Base { _dir: dir, path }
}

fn env(pairs: &[(&str, &str)]) -> SourceEnv {
    SourceEnv::from_lookup(|key| {
        pairs
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| OsString::from(*value))
    })
}

fn mise(tool: &str) -> InstallSource {
    InstallSource::Mise {
        tool: tool.to_owned(),
    }
}

#[test]
fn mise_under_the_default_home_root_names_the_github_tool() {
    let root = base("home-root");
    let exe = root.join(".local/share/mise/installs/github-followcat-backlot/0.1.6/backlot");
    let source_env = env(&[("HOME", root.to_str().unwrap())]);
    assert_eq!(detect(&exe, &source_env), mise("github:followcat/backlot"));
}

#[test]
fn the_configured_mise_installs_dir_is_a_root() {
    let root = base("installs-dir");
    let installs = root.join("custom/installs");
    let exe = installs.join("github-followcat-backlot/0.1.6/backlot");
    let source_env = env(&[("MISE_INSTALLS_DIR", installs.to_str().unwrap())]);
    assert_eq!(detect(&exe, &source_env), mise("github:followcat/backlot"));
}

#[test]
fn the_mise_data_dir_has_an_installs_root() {
    let root = base("data-dir");
    let data = root.join("mise-data");
    let exe = data.join("installs/github-followcat-backlot/0.1.6/backlot");
    let source_env = env(&[("MISE_DATA_DIR", data.to_str().unwrap())]);
    assert_eq!(detect(&exe, &source_env), mise("github:followcat/backlot"));
}

#[test]
fn xdg_data_home_has_a_mise_installs_root() {
    let root = base("xdg");
    let xdg = root.join("xdg");
    let exe = xdg.join("mise/installs/github-followcat-backlot/0.1.6/backlot");
    let source_env = env(&[("XDG_DATA_HOME", xdg.to_str().unwrap())]);
    assert_eq!(detect(&exe, &source_env), mise("github:followcat/backlot"));
}

#[test]
fn mise_install_names_map_to_their_backend_tool() {
    let source_env = env(&[("HOME", "/home/user")]);
    for (dir, tool) in [
        ("github-followcat-backlot", "github:followcat/backlot"),
        ("ubi-followcat-backlot", "ubi:followcat/backlot"),
        ("aqua-followcat-backlot", "aqua:followcat/backlot"),
        ("cargo-backlot", "cargo:backlot"),
        ("plain-tool", "plain-tool"),
    ] {
        let exe = PathBuf::from(format!(
            "/home/user/.local/share/mise/installs/{dir}/0.1.6/backlot"
        ));
        assert_eq!(detect(&exe, &source_env), mise(tool), "{dir}");
    }
}

#[test]
fn homebrew_is_found_through_a_symlinked_bin() {
    let root = base("brew");
    let keg = root.join("Cellar/backlot/0.1.11/bin");
    std::fs::create_dir_all(&keg).unwrap();
    std::fs::write(keg.join("backlot"), b"#!/bin/sh\n").unwrap();
    std::fs::create_dir_all(root.join("bin")).unwrap();
    let link = root.join("bin/backlot");
    std::os::unix::fs::symlink(keg.join("backlot"), &link).unwrap();
    let exe = canonical_exe(&link);
    assert_eq!(exe, keg.join("backlot"));
    assert_eq!(detect(&exe, &SourceEnv::default()), InstallSource::Homebrew);
}

#[test]
fn a_nix_store_path_is_nix_without_touching_the_disk() {
    let exe = Path::new("/nix/store/abc123-backlot-0.1.11/bin/backlot");
    assert_eq!(detect(exe, &SourceEnv::default()), InstallSource::Nix);
}

#[test]
fn a_cargo_bin_is_cargo_under_cargo_home_or_the_default_home() {
    let root = base("cargo");
    let cargo_home = root.join("cargo-home");
    let exe = cargo_home.join("bin/backlot");
    let source_env = env(&[("CARGO_HOME", cargo_home.to_str().unwrap())]);
    assert_eq!(detect(&exe, &source_env), InstallSource::Cargo);
    let home = root.join("home");
    let exe = home.join(".cargo/bin/backlot");
    let source_env = env(&[("HOME", home.to_str().unwrap())]);
    assert_eq!(detect(&exe, &source_env), InstallSource::Cargo);
}

#[test]
fn target_debug_release_and_triple_dirs_are_dev_builds() {
    for path in [
        "/home/user/src/backlot/target/debug/backlot",
        "/home/user/src/backlot/target/release/backlot",
        "/home/user/src/backlot/target/x86_64-unknown-linux-gnu/release/backlot",
    ] {
        assert_eq!(
            detect(Path::new(path), &SourceEnv::default()),
            InstallSource::DevBuild,
            "{path}"
        );
    }
}

#[test]
fn a_plain_home_bin_install_is_direct() {
    let root = base("direct");
    let home = root.join("home");
    let exe = home.join("bin/backlot");
    let source_env = env(&[("HOME", home.to_str().unwrap())]);
    assert_eq!(detect(&exe, &source_env), InstallSource::Direct);
}

#[test]
fn a_path_that_only_mentions_mise_stays_direct() {
    let source_env = env(&[("HOME", "/home/user")]);
    for path in [
        "/home/user/mise-tools/backlot",
        "/opt/mise/backlot",
        "/home/user/projects/mise/installs/backlot",
        "/home/user/.local/share/mise/backlot",
    ] {
        assert_eq!(
            detect(Path::new(path), &source_env),
            InstallSource::Direct,
            "{path}"
        );
    }
}

#[test]
fn each_channel_names_its_own_refusal() {
    assert_eq!(
        mise("github:followcat/backlot").refusal().as_deref(),
        Some("backlot is managed by mise; run: mise upgrade --bump github:followcat/backlot")
    );
    assert_eq!(
        InstallSource::Homebrew.refusal().as_deref(),
        Some("backlot is managed by Homebrew; run: brew upgrade backlot")
    );
    assert_eq!(
        InstallSource::Nix.refusal().as_deref(),
        Some(
            "backlot is managed by Nix; update it through the Nix configuration that installed it"
        )
    );
    assert_eq!(
        InstallSource::Cargo.refusal().as_deref(),
        Some("backlot was installed with cargo; reinstall it with cargo the way you installed it")
    );
    assert_eq!(
        InstallSource::DevBuild.refusal().as_deref(),
        Some("backlot is a dev build and is never upgraded; rebuild it from your checkout")
    );
    assert_eq!(InstallSource::Direct.refusal(), None);
}

#[test]
fn only_the_managed_and_dev_channels_refuse_an_upgrade() {
    assert!(mise("github:followcat/backlot").refusal().is_some());
    assert!(InstallSource::Homebrew.refusal().is_some());
    assert!(InstallSource::Nix.refusal().is_some());
    assert!(InstallSource::Cargo.refusal().is_some());
    assert!(InstallSource::DevBuild.refusal().is_some());
    assert!(!InstallSource::Direct.refusal().is_some());
}

#[test]
fn the_card_names_each_channel_and_offers_its_command_as_the_chip() {
    assert_eq!(
        mise("github:followcat/backlot").card_phrase().as_deref(),
        Some("managed by mise: update it there")
    );
    assert_eq!(
        mise("github:followcat/backlot").card_chip().as_deref(),
        Some("mise upgrade --bump github:followcat/backlot")
    );
    assert_eq!(
        InstallSource::Homebrew.card_phrase().as_deref(),
        Some("managed by Homebrew: update it there")
    );
    assert_eq!(
        InstallSource::Homebrew.card_chip().as_deref(),
        Some("brew upgrade backlot")
    );
    assert_eq!(
        InstallSource::Nix.card_phrase().as_deref(),
        Some("managed by Nix: update it in your Nix config")
    );
    assert_eq!(InstallSource::Nix.card_chip(), None);
    assert_eq!(
        InstallSource::Cargo.card_phrase().as_deref(),
        Some("installed with cargo: reinstall it with cargo")
    );
    assert_eq!(InstallSource::Cargo.card_chip(), None);
    assert_eq!(
        InstallSource::DevBuild.card_phrase().as_deref(),
        Some("dev build: rebuild it from your checkout")
    );
    assert_eq!(InstallSource::DevBuild.card_chip(), None);
    assert_eq!(InstallSource::Direct.card_phrase(), None);
    assert_eq!(InstallSource::Direct.card_chip(), None);
}

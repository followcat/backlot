// Derived from herdr src/update.rs (Apache-2.0): the managed-install
// refusal and its per-channel update command, with hc-tui's own paths
// and commands.
//! Which channel the running hc-tui came from, and whether `--upgrade`
//! and the update card may replace that binary in place.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// How the running binary was installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallSource {
    /// Installed by mise; `tool` is mise's name for it, e.g. `github:owner/repo`.
    Mise { tool: String },
    /// Installed by Homebrew into its Cellar.
    Homebrew,
    /// Installed by Nix into `/nix/store`.
    Nix,
    /// Installed by `cargo install` into `$CARGO_HOME/bin`.
    Cargo,
    /// Built in a checkout under `target/`.
    DevBuild,
    /// Anything else: upgraded in place by the official installer.
    Direct,
}

/// The environment detection reads, resolved to the directories it
/// compares against. Empty values count as unset.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SourceEnv {
    mise_roots: Vec<PathBuf>,
    cargo_bin: Option<PathBuf>,
}

impl SourceEnv {
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<OsString>) -> Self {
        let dir = |key: &str| {
            lookup(key)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        let mise_roots = [
            dir("MISE_INSTALLS_DIR"),
            dir("MISE_DATA_DIR").map(|data| data.join("installs")),
            dir("XDG_DATA_HOME").map(|data| data.join("mise/installs")),
            dir("HOME").map(|home| home.join(".local/share/mise/installs")),
        ]
        .into_iter()
        .flatten()
        .collect();
        let cargo_home = dir("CARGO_HOME").or_else(|| dir("HOME").map(|home| home.join(".cargo")));
        Self {
            mise_roots,
            cargo_bin: cargo_home.map(|home| home.join("bin")),
        }
    }

    /// Resolves each root and bin dir that exists, so a symlinked
    /// `$HOME` or data dir still matches the canonical executable.
    #[must_use]
    pub fn canonicalized(self) -> Self {
        let canonical = |path: PathBuf| path.canonicalize().unwrap_or(path);
        Self {
            mise_roots: self.mise_roots.into_iter().map(canonical).collect(),
            cargo_bin: self.cargo_bin.map(canonical),
        }
    }
}

/// The channel of the running binary, read from its canonical path and
/// the environment `lookup` provides.
pub fn current(lookup: impl Fn(&str) -> Option<OsString>) -> InstallSource {
    let Ok(exe) = std::env::current_exe() else {
        return InstallSource::Direct;
    };
    detect(
        &crate::update_install::canonical_exe(&exe),
        &SourceEnv::from_lookup(lookup).canonicalized(),
    )
}

/// Classifies a canonical executable path. The first matching channel
/// wins, in the order mise, Homebrew, Nix, cargo, dev build.
pub fn detect(exe: &Path, env: &SourceEnv) -> InstallSource {
    if let Some(tool) = mise_tool(exe, env) {
        InstallSource::Mise { tool }
    } else if is_homebrew(exe) {
        InstallSource::Homebrew
    } else if exe.starts_with("/nix/store") {
        InstallSource::Nix
    } else if env
        .cargo_bin
        .as_deref()
        .is_some_and(|bin| exe.parent() == Some(bin))
    {
        InstallSource::Cargo
    } else if is_dev_build(exe) {
        InstallSource::DevBuild
    } else {
        InstallSource::Direct
    }
}

/// `<root>/<tool>/<version>/…/<binary>` under any mise installs root.
fn mise_tool(exe: &Path, env: &SourceEnv) -> Option<String> {
    env.mise_roots.iter().find_map(|root| {
        let mut parts = exe
            .strip_prefix(root)
            .ok()?
            .components()
            .filter_map(|part| part.as_os_str().to_str());
        let tool = parts.next()?;
        (parts.count() >= 2).then(|| mise_tool_name(tool))
    })
}

/// mise names a tool directory `<backend>-<owner>-<repo>`. The owner ends
/// at the first dash after the backend, so an owner that contains a dash
/// comes out wrong; other names are kept as they are.
fn mise_tool_name(dir: &str) -> String {
    if let Some(name) = dir.strip_prefix("cargo-") {
        return format!("cargo:{name}");
    }
    for backend in ["github", "ubi", "aqua"] {
        let rest = dir
            .strip_prefix(backend)
            .and_then(|rest| rest.strip_prefix('-'));
        if let Some((owner, repo)) = rest.and_then(|rest| rest.split_once('-')) {
            return format!("{backend}:{owner}/{repo}");
        }
    }
    dir.to_owned()
}

/// A Homebrew keg: `…/Cellar/hc-tui/<version>/…`.
fn is_homebrew(exe: &Path) -> bool {
    let parts: Vec<_> = exe
        .components()
        .filter_map(|part| part.as_os_str().to_str())
        .collect();
    parts.windows(2).any(|pair| pair == ["Cellar", "hc-tui"])
}

/// A checkout build: `target/debug` or `target/release`, or the same
/// under a cross-target triple (`target/<triple>/release`).
fn is_dev_build(exe: &Path) -> bool {
    let parts: Vec<_> = exe
        .components()
        .filter_map(|part| part.as_os_str().to_str())
        .collect();
    let profile = |part: Option<&&str>| matches!(part, Some(&("debug" | "release")));
    parts.iter().enumerate().any(|(i, part)| {
        *part == "target" && (profile(parts.get(i + 1)) || profile(parts.get(i + 2)))
    })
}

impl InstallSource {
    /// The `--upgrade` refusal line; `None` for a direct install.
    pub fn refusal(&self) -> Option<String> {
        Some(match self {
            Self::Mise { tool } => format!("hc-tui is managed by mise; run: {}", mise_command(tool)),
            Self::Homebrew => format!("hc-tui is managed by Homebrew; run: {HOMEBREW_COMMAND}"),
            Self::Nix => "hc-tui is managed by Nix; update it through the Nix configuration that installed it".into(),
            Self::Cargo => "hc-tui was installed with cargo; reinstall it with cargo the way you installed it".into(),
            Self::DevBuild => "hc-tui is a dev build and is never upgraded; rebuild it from your checkout".into(),
            Self::Direct => return None,
        })
    }

    /// The callout line that names the channel on the update card.
    pub fn card_phrase(&self) -> Option<String> {
        Some(match self {
            Self::Mise { .. } => "managed by mise: update it there".into(),
            Self::Homebrew => "managed by Homebrew: update it there".into(),
            Self::Nix => "managed by Nix: update it in your Nix config".into(),
            Self::Cargo => "installed with cargo: reinstall it with cargo".into(),
            Self::DevBuild => "dev build: rebuild it from your checkout".into(),
            Self::Direct => return None,
        })
    }

    /// The command the card shows, for the channels that have one.
    pub fn card_chip(&self) -> Option<String> {
        match self {
            Self::Mise { tool } => Some(mise_command(tool)),
            Self::Homebrew => Some(HOMEBREW_COMMAND.into()),
            _ => None,
        }
    }
}

const HOMEBREW_COMMAND: &str = "brew upgrade hc-tui";

/// `--bump` rewrites an exact pin in the mise config. A plain
/// `mise upgrade` stays inside the version the config requests, so an
/// exact pin such as `= "0.1.6"` would leave the install where it is.
fn mise_command(tool: &str) -> String {
    format!("mise upgrade --bump {tool}")
}

#[cfg(test)]
#[path = "install_source_tests.rs"]
mod tests;

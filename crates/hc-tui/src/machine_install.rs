//! The per-machine install/upgrade plumbing (U6c): the read-only
//! env probe a remote machine gets before the installer runs, its
//! parse, and the installer pipeline a `terminal.run` popup shows.
//! The version is re-validated through `update_check::semver` here
//! — the one place a release string enters a shell line — and the
//! install dir is single-quoted through `update_install::shell_quote`.

use std::collections::BTreeMap;

use crate::update_install::shell_quote;

/// The env vars `install_source::SourceEnv::from_lookup` reads —
/// probed on the target machine so a remote binary classifies the
/// same way the local one does.
pub(crate) const ENV_KEYS: &[&str] = &[
    "HOME",
    "MISE_DATA_DIR",
    "MISE_INSTALLS_DIR",
    "XDG_DATA_HOME",
    "CARGO_HOME",
];

/// The read-only classification probe: print `EXE=<canonical
/// backlot>` when the locate finds a binary (the same chain
/// `RemoteRunner::LOCATE_SCRIPT` uses — `command -v`, the login
/// shell's lookup, `~/.local/bin/backlot`), then one `ENV k=v` line
/// per env var, each value canonicalized with `cd … && pwd -P` so a
/// symlinked `$HOME` still matches a canonical exe path.
/// `readlink -f`/`realpath` resolve the binary itself — a
/// `~/.local/bin/backlot` symlink classifies by where it points.
pub(crate) fn env_probe_script() -> String {
    let keys = ENV_KEYS.join(" ");
    format!(
        "p=$(command -v backlot 2>/dev/null); \
[ -z \"$p\" ] && s=${{SHELL:-/bin/sh}} && p=$(\"$s\" -lc 'command -v backlot' </dev/null 2>/dev/null | tail -n 1); \
[ -z \"$p\" ] && [ -x \"$HOME/.local/bin/backlot\" ] && p=\"$HOME/.local/bin/backlot\"; \
if [ -n \"$p\" ]; then \
rp=$(readlink -f \"$p\" 2>/dev/null) || rp=$(realpath \"$p\" 2>/dev/null) || rp=\"$p\"; \
printf 'EXE=%s\\n' \"$rp\"; \
fi; \
for k in {keys}; do \
eval \"v=\\${{$k}}\"; \
if [ -n \"$v\" ]; then \
vc=$(cd \"$v\" 2>/dev/null && pwd -P) || vc=\"$v\"; \
printf 'ENV %s=%s\\n' \"$k\" \"$vc\"; \
fi; \
done; \
exit 0"
    )
}

/// The probe's answer: the canonical exe path when one printed,
/// plus the env map `SourceEnv::from_lookup` reads. Values were
/// already canonicalized on the remote, so no `canonicalized()`
/// pass — the local filesystem must never resolve a remote path.
pub(crate) fn parse_env_probe(text: &str) -> (Option<String>, BTreeMap<String, String>) {
    let mut exe = None;
    let mut env = BTreeMap::new();
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("EXE=") {
            let path = path.trim();
            if path.starts_with('/') {
                exe = Some(path.to_owned());
            }
        } else if let Some(rest) = line.strip_prefix("ENV ")
            && let Some((key, value)) = rest.split_once('=')
            && ENV_KEYS.contains(&key)
        {
            env.insert(key.to_owned(), value.to_owned());
        }
    }
    (exe, env)
}

/// The directory `BACKLOT_INSTALL_DIR` gets on an upgrade: the
/// canonical exe's parent. `None` on a fresh install — the
/// installer's own `~/.local/bin` default then applies.
pub(crate) fn install_dir(exe: Option<&str>) -> Option<String> {
    let exe = exe?;
    std::path::Path::new(exe)
        .parent()
        .map(|dir| dir.to_string_lossy().into_owned())
}

/// The pipeline `terminal.run` shows in the popup: `curl … |
/// env … sh` — `BACKLOT_SKIP_CORE=1` always (the installer can
/// never touch `hc`), the pinned `BACKLOT_VERSION` rebuilt from
/// parsed numbers only, and `BACKLOT_INSTALL_DIR` quoted when the
/// upgrade targets a known binary's directory.
///
/// `Err` when `version` fails `update_check::semver` — a hostile
/// string (`v9.9.9.;id;`) can never reach the shell.
pub(crate) fn install_line(version: &str, install_dir: Option<&str>) -> Result<String, String> {
    let (major, minor, patch) = crate::update_check::semver(version)
        .ok_or_else(|| format!("refusing to install unparseable version {version:?}"))?;
    let pinned = format!("v{major}.{minor}.{patch}");
    let mut env = format!(
        "BACKLOT_VERSION={} BACKLOT_SKIP_CORE=1",
        shell_quote(&pinned)
    );
    if let Some(dir) = install_dir {
        env.push_str(&format!(" BACKLOT_INSTALL_DIR={}", shell_quote(dir)));
    }
    Ok(format!(
        "curl -fsSL {} | {env} sh",
        crate::update_run::INSTALL_URL
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_prints_exe_and_canonical_env() {
        let text = "EXE=/home/user/.local/share/mise/installs/backlot/0.2.4/bin/backlot\n\
ENV HOME=/home/user\nENV MISE_DATA_DIR=/home/user/.local/share/mise\nENV CARGO_HOME=\n";
        let (exe, env) = parse_env_probe(text);
        assert_eq!(
            exe.as_deref(),
            Some("/home/user/.local/share/mise/installs/backlot/0.2.4/bin/backlot")
        );
        assert_eq!(env.get("HOME").map(String::as_str), Some("/home/user"));
        assert_eq!(
            env.get("MISE_DATA_DIR").map(String::as_str),
            Some("/home/user/.local/share/mise")
        );
    }

    #[test]
    fn the_probe_ignores_stray_and_relative_lines() {
        let (exe, env) = parse_env_probe("noise\nEXE=backlot\nENV BOGUS=x\nENV HOME=/home/user\n");
        assert_eq!(exe, None, "a relative EXE is not a found binary");
        assert!(!env.contains_key("BOGUS"));
        assert_eq!(env.get("HOME").map(String::as_str), Some("/home/user"));
    }

    #[test]
    fn the_install_line_pins_skip_core_and_dir() {
        let line = install_line("0.2.4", Some("/home/user/.local/bin")).unwrap();
        assert_eq!(
            line,
            format!(
                "curl -fsSL {} | BACKLOT_VERSION=v0.2.4 BACKLOT_SKIP_CORE=1 BACKLOT_INSTALL_DIR=/home/user/.local/bin sh",
                crate::update_run::INSTALL_URL
            )
        );
        let line = install_line("v0.2.4", Some("/home/o'neil/bin")).unwrap();
        assert!(
            line.contains("BACKLOT_INSTALL_DIR='/home/o'\\''neil/bin'"),
            "the dir is single-quoted: {line}"
        );
        let line = install_line("0.2.4", None).unwrap();
        assert!(
            !line.contains("BACKLOT_INSTALL_DIR"),
            "a fresh install defaults"
        );
    }

    #[test]
    fn a_hostile_version_never_reaches_the_shell() {
        for hostile in [
            "v9.9.9.;id;",
            "v1.2.3$(x)",
            "v1..2",
            "garbage",
            "v1.2.3-;id;",
        ] {
            assert!(
                install_line(hostile, Some("/tmp/x")).is_err(),
                "{hostile:?} must refuse"
            );
        }
    }

    #[test]
    fn install_dir_is_the_exes_parent() {
        assert_eq!(
            install_dir(Some("/home/user/.local/bin/backlot")).as_deref(),
            Some("/home/user/.local/bin")
        );
        assert_eq!(install_dir(Some("/backlot")), Some("/".into()));
        assert_eq!(install_dir(None), None);
    }
}

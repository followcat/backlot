//! The one place a runner's launch binary is decided (Ux): a
//! configured `[runner_env.<runner>]` PATH IS the whole launch
//! PATH — the binary must be on it and no other location counts;
//! without one, detection's order stands — inherited PATH, the
//! login shell's PATH, the well-known install dirs — and a hit
//! outside the inherited PATH launches through the `[runner_env]`
//! write `ensure_runner_paths` records at `studio new`. The
//! picker's "detected" list and `studio new --dry-run` both ask
//! it, so the set offered is the set that can launch.

use crate::cli::Env;
use crate::runners::detect::{self, Found, Scope, Source};
use crate::runners::{probe, tools_dir};

/// `runner`'s resolved launch binary — `Found` carries the path a
/// launch would get plus where the hit came from — or a refusal
/// naming the PATH that was searched, actionable on its own.
pub(crate) fn launch_binary(
    runner: &str,
    config_text: &str,
    scope: &Scope,
) -> Result<Found, String> {
    if runner == "fake" {
        let path = tools_dir().join("fake-runner.sh");
        return if path.is_file() {
            Ok(Found {
                path,
                version: None,
                source: Source::Inherited,
            })
        } else {
            Err(format!(
                "runner 'fake' is not installed: tools/fake-runner.sh missing under {}",
                tools_dir().display()
            ))
        };
    }
    let Some(bin) = probe::launch_program(runner) else {
        return Err(format!(
            "runner '{runner}' is not installed: no launch program"
        ));
    };
    match crate::config::runner_env_in(config_text, runner) {
        Err(e) => Err(e),
        Ok(map) => match map.get("PATH") {
            // The configured PATH replaces the inherited one at
            // launch, so it is the only place that counts.
            Some(path) => probe::find_on_path(bin, Some(path))
                .map(|p| Found {
                    path: p,
                    version: None,
                    source: Source::Inherited,
                })
                .ok_or_else(|| format!("runner '{runner}' is not installed: searched PATH={path}")),
            None => detect::find(runner, scope).ok_or_else(|| {
                let inherited = (scope.env)("PATH").unwrap_or_else(|| "(unset)".into());
                format!(
                    "runner '{runner}' is not installed: searched PATH={inherited}, \
                     plus the login shell's PATH and the known install dirs"
                )
            }),
        },
    }
}

/// The PATH `path`'s hit launches on when it is not the inherited
/// one — the binary's own directory first, like the `[runner_env]`
/// write gives a role (a node CLI needs its `node` next door).
pub(crate) fn launch_path(found: &Found, env: &Env) -> String {
    match env.get("PATH") {
        Some(inherited) if !inherited.is_empty() => {
            format!("{}:{inherited}", found.bin_dir().display())
        }
        _ => found.bin_dir().to_string_lossy().into_owned(),
    }
}

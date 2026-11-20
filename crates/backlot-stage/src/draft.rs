//! The `studio new --dry-run` check (D19): everything `studio new`
//! itself would refuse — the cast file's own rules — plus what only
//! a dry run can ask: the name is free, the bound path's shape is
//! bindable, every named runner is supported and installed, and the
//! caps can seat the fixed roles. Nothing is recorded; nothing is
//! created. `hollywood propose studio-new` accepts only a draft that
//! passes.

use std::collections::BTreeSet;
use std::path::Path;

use crate::cli::{Env, Fail, check_studio_name};
use crate::commands::read_cast;
pub use crate::paths::Binding;
use crate::store;

/// The draft, checked end to end. `base` is the state directory the
/// studio would live under; `init`/`no_git` are the `--init`/`--no-git`
/// flags, which only a plain directory may use.
pub fn check(
    base: &Path,
    name: &str,
    repo: &str,
    cast_path: &Path,
    env: &Env,
    init: bool,
    no_git: bool,
) -> Result<Binding, Fail> {
    check_studio_name(name)
        .map_err(|_| Fail::usage(format!("bad studio name '{name}': want [a-z0-9_-]{{1,16}}")))?;
    // `hollywood` names the overseer's ledger beside `studios/` —
    // a studio can never take it (D13).
    if name == "hollywood" {
        return Err(Fail::refused(
            "the name 'hollywood' is reserved for the overseer",
        ));
    }
    if store::studio_dir(base, name).join("events.jsonl").is_file() {
        return Err(Fail::refused(format!(
            "a studio named '{name}' already exists"
        )));
    }
    let binding = crate::paths::check(repo, init, no_git)?;
    let cast = read_cast(cast_path)?;
    // The check declarations compile here too — a broken `run` would
    // be the stage's first attention otherwise. Declared checks need
    // one repository (D10) — a workspace or a no-git studio has none
    // to detach a check worktree from.
    crate::checks::declarations(&cast.check_env, &cast.checks)?;
    if !cast.checks.is_empty() && (binding.no_git || !binding.members.is_empty()) {
        return Err(Fail::refused(
            "[[checks]] need a single git repository — a workspace or no-git studio cannot run them",
        ));
    }
    // Every runner the draft names must have an adapter and a program
    // this shell can launch.
    let mut named = BTreeSet::new();
    for a in [&cast.producer, &cast.director, &cast.writer] {
        named.insert(a.runner.as_str());
    }
    for c in &cast.candidates {
        named.insert(c.runner.as_str());
    }
    // The PATH a launch would get: a configured
    // `[runner_env.<runner>]` PATH replaces the inherited one — the
    // wizard's own write counts as installed (U5).
    let config_text = std::fs::read_to_string(base.join("config.toml")).unwrap_or_default();
    for runner in named {
        if !crate::runners::supported(runner) {
            return Err(Fail::refused(format!(
                "runner '{runner}' is not supported (supported: fake, claude, codex, devin, opencode, cursor, cmd, cline)"
            )));
        }
        let path = crate::config::runner_env_in(&config_text, runner)
            .ok()
            .and_then(|env| env.get("PATH").cloned())
            .or_else(|| env.get("PATH").map(str::to_string));
        if !crate::runners::installed(runner, path.as_deref()) {
            let what = if runner == "fake" {
                "tools/fake-runner.sh"
            } else {
                runner
            };
            return Err(Fail::refused(format!(
                "runner '{runner}' is not installed: {what} not found on PATH"
            )));
        }
    }
    Ok(binding)
}

//! The `studio new --dry-run` check (D19): everything `studio new`
//! itself would refuse — the cast file's own rules — plus what only
//! a dry run can ask: the name is free, the bound path's shape is
//! bindable, every named runner is supported and installed, and the
//! caps can seat the fixed roles. Nothing studio-side is recorded;
//! the `[runner_env]` PATH write a seated runner's launch needs is
//! machine config — a dry run applies it exactly like `studio new`
//! does, never overwriting an existing PATH. `hollywood propose
//! studio-new` accepts only a draft that passes.

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
    // The check runs where the repository lives; a remote draft never
    // canonicalizes a hub path against the client.
    let repo = crate::args_studio::canonical_repo(repo);
    let binding = crate::paths::check(&repo, init, no_git)?;
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
    // Every runner the draft names must have an adapter and a launch
    // file that resolves under the PATH a launch gets.
    let named = crate::commands::cast_runners(&cast);
    check_runners(base, &named, env)?;
    Ok(binding)
}

/// The runner half of the `studio new --dry-run` check, shared with
/// `backlot recast` and a recast card's proposal: every named runner
/// has an adapter, seated ones detection reached only outside the
/// inherited PATH earn their `[runner_env]` write, and the launch PATH
/// resolves each one. An existing PATH is never rewritten.
pub(crate) fn check_runners(
    base: &Path,
    named: &std::collections::BTreeSet<String>,
    env: &Env,
) -> Result<(), Fail> {
    for runner in named {
        if !crate::runners::supported(runner) {
            return Err(Fail::refused(format!(
                "runner '{runner}' is not supported (supported: fake, claude, codex, devin, opencode, cursor, cmd, cline, agy)"
            )));
        }
    }
    let scope = crate::runners::detect::scope(env);
    let mut stderr = std::io::stderr();
    crate::wizard_paths::ensure_runner_paths(
        base,
        named,
        env,
        &mut |runner| crate::runners::detect::find(runner, &scope),
        &mut stderr,
    )?;
    // A configured `[runner_env.<runner>]` PATH is the whole launch
    // PATH, and without one detection's order stands — the picker's
    // "detected" list and this check answer the same way.
    let config_text = std::fs::read_to_string(base.join("config.toml")).unwrap_or_default();
    for runner in named {
        crate::runners::resolve::launch_binary(runner, &config_text, &scope)
            .map_err(Fail::refused)?;
    }
    Ok(())
}

/// Dry-run notes when a cast requests `"worktree"` on runners whose
/// `"worktree"` and `"standard"` profiles emit the same settings/flags.
pub(crate) fn dry_run_notes(cast_path: &Path) -> Result<Vec<String>, Fail> {
    let cast = read_cast(cast_path)?;
    let mut by_runner: Vec<(String, Vec<String>)> = Vec::new();
    let mut record = |role: &str, runner: &str, profile: &str| {
        if profile == "worktree" && crate::runners::IDENTICAL_WORKTREE_RUNNERS.contains(&runner) {
            if let Some((_, roles)) = by_runner.iter_mut().find(|(r, _)| r == runner) {
                roles.push(role.to_string());
            } else {
                by_runner.push((runner.to_string(), vec![role.to_string()]));
            }
        }
    };
    record(
        "producer",
        &cast.producer.runner,
        &cast.producer.permission_profile,
    );
    record(
        "director",
        &cast.director.runner,
        &cast.director.permission_profile,
    );
    record(
        "writer",
        &cast.writer.runner,
        &cast.writer.permission_profile,
    );
    for c in &cast.candidates {
        record(&c.name, &c.runner, &c.permission_profile);
    }
    Ok(by_runner
        .into_iter()
        .map(|(runner, roles)| {
            format!(
                "permission_profile = \"worktree\" is identical to \"standard\" on {runner} ({})",
                roles.join(", ")
            )
        })
        .collect())
}

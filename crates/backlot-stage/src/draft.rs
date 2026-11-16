//! The `studio new --dry-run` check (D19): everything `studio new`
//! itself would refuse — the cast file's own rules — plus what only
//! a dry run can ask: the name is free, the repo is a git repository,
//! every named runner is supported and installed, and the caps can
//! seat the fixed roles. Nothing is recorded; nothing is created.
//! `hollywood propose studio-new` accepts only a draft that passes.

use std::collections::BTreeSet;
use std::path::Path;

use crate::cli::{Env, Fail, check_studio_name};
use crate::commands::read_cast;
use crate::store;
use crate::workareas;

/// The draft, checked end to end. `base` is the state directory the
/// studio would live under.
pub fn check(base: &Path, name: &str, repo: &str, cast_path: &Path, env: &Env) -> Result<(), Fail> {
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
    // The repo is a git repository the user owns — ask git itself so
    // linked worktrees and bare checkouts count alike.
    let out = workareas::real_git(&["rev-parse", "--git-dir", "--end-of-options"], repo);
    if out.code != 0 {
        return Err(Fail::refused(format!(
            "--repo '{repo}' is not a git repository ({})",
            workareas::err_line(&out)
        )));
    }
    let cast = read_cast(cast_path)?;
    // The check declarations compile here too — a broken `run` would
    // be the stage's first attention otherwise.
    crate::checks::declarations(&cast.check_env, &cast.checks)?;
    // Every runner the draft names must have an adapter and a program
    // this shell can launch.
    let mut named = BTreeSet::new();
    for a in [&cast.producer, &cast.director, &cast.writer] {
        named.insert(a.runner.as_str());
    }
    for c in &cast.candidates {
        named.insert(c.runner.as_str());
    }
    for runner in named {
        if !crate::runners::supported(runner) {
            return Err(Fail::refused(format!(
                "runner '{runner}' is not supported (supported: fake, claude, codex, devin, opencode, cursor, cmd)"
            )));
        }
        if !crate::runners::installed(runner, env.get("PATH")) {
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
    Ok(())
}

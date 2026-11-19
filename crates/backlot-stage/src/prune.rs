//! `backlot studio prune`: reap a finished studio's worktrees and
//! `backlot/<studio>/*` branches. The ledger, `reports/`, `messages/`
//! and `attention/` are never touched — `backlot records` and
//! `backlot status` keep the full history.
//!
//! Safety first: a live studio is refused, a clean worktree goes with
//! `git worktree remove`, a dirty one is kept unless `--force`, and a
//! branch is deleted with `git branch -d` (never `-D` without
//! `--force`) so unmerged integration work is never lost. `--dry-run`
//! reads git and prints the plan without changing anything.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::cli::Fail;
use crate::store;
use crate::workareas::{GitRun, err_line, real_git};

/// One thing prune did — or, in a dry run, would do.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Step {
    RemovedWorktree(String),
    PrunedWorktree(String),
    KeptWorktree(String, String),
    DeletedBranch(String),
    KeptBranch(String, String),
}

/// The parsed `worktree list --porcelain` rows: path and branch.
fn parse_worktrees(list: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let mut path: Option<String> = None;
    let mut branch: Option<String> = None;
    let flush = |out: &mut Vec<(String, Option<String>)>,
                 path: &mut Option<String>,
                 branch: &mut Option<String>| {
        if let Some(p) = path.take() {
            out.push((p, branch.take()));
        }
        *branch = None;
    };
    for line in list.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            flush(&mut out, &mut path, &mut branch);
            path = Some(p.to_string());
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            branch = Some(b.to_string());
        } else if line.is_empty() {
            flush(&mut out, &mut path, &mut branch);
        }
    }
    flush(&mut out, &mut path, &mut branch);
    out
}

/// `true` when `path` sits strictly under `root`, with symlinked state
/// directories resolved on both sides.
fn under(path: &Path, root: &Path) -> bool {
    let strictly_under = |p: &Path, r: &Path| p != r && p.starts_with(r);
    if strictly_under(path, root) {
        return true;
    }
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(p), Ok(r)) => strictly_under(&p, &r),
        _ => false,
    }
}

/// A path as it reads inside the studio directory when it can.
fn shown(path: &Path, dir: &Path) -> String {
    path.strip_prefix(dir).unwrap_or(path).display().to_string()
}

/// The `backlot/<studio>/` branch prefix — the only branches prune may
/// ever touch.
fn branch_prefix(studio: &str) -> String {
    format!("backlot/{studio}/")
}

/// Work the finished studio's `work/` trees first, then its branches.
/// `git` is injected so the planning tests stay fast and pure; the
/// caller passes [`real_git`] in production.
pub(crate) fn prune_studio(
    git: &mut GitRun<'_>,
    studio: &str,
    dir: &Path,
    repo: &str,
    dry_run: bool,
    force: bool,
) -> Result<Vec<Step>, Fail> {
    let list = git(&["-C", repo, "worktree", "list", "--porcelain"], repo);
    if list.code != 0 {
        return Err(Fail::refused(format!(
            "studio '{studio}': git worktree list: {}",
            err_line(&list)
        )));
    }
    let work_root = dir.join("work");
    let mut steps = Vec::new();
    let mut missing = Vec::new();
    // Branches whose worktree is staying put: git cannot delete a
    // branch still checked out there, so a dry run reports it kept
    // rather than predicting a deletion the real pass would refuse.
    let mut kept_branches = BTreeSet::new();
    for (path, branch) in parse_worktrees(&list.stdout) {
        let tree = PathBuf::from(&path);
        if !under(&tree, &work_root) {
            continue;
        }
        let label = shown(&tree, dir);
        if !tree.exists() {
            missing.push(label);
            continue;
        }
        let status = git(&["status", "--porcelain"], &path);
        if status.code != 0 {
            if let Some(b) = branch {
                kept_branches.insert(b);
            }
            steps.push(Step::KeptWorktree(
                label,
                format!("git status failed: {}", err_line(&status)),
            ));
            continue;
        }
        let dirty = !status.stdout.trim().is_empty();
        if dirty && !force {
            if let Some(b) = branch {
                kept_branches.insert(b);
            }
            steps.push(Step::KeptWorktree(label, "uncommitted changes".into()));
            continue;
        }
        if dry_run {
            steps.push(Step::RemovedWorktree(label));
            continue;
        }
        let mut argv = vec!["-C", repo, "worktree", "remove"];
        if dirty {
            argv.push("--force");
        }
        argv.push(&path);
        let out = git(&argv, repo);
        if out.code == 0 {
            steps.push(Step::RemovedWorktree(label));
        } else {
            if let Some(b) = branch {
                kept_branches.insert(b);
            }
            steps.push(Step::KeptWorktree(label, err_line(&out)));
        }
    }
    // A registered worktree whose folder is gone needs only
    // `worktree prune`, once for all of them.
    if !missing.is_empty() {
        if dry_run {
            steps.extend(missing.into_iter().map(Step::PrunedWorktree));
        } else {
            let out = git(&["-C", repo, "worktree", "prune"], repo);
            if out.code == 0 {
                steps.extend(missing.into_iter().map(Step::PrunedWorktree));
            } else {
                let why = format!("git worktree prune failed: {}", err_line(&out));
                steps.extend(
                    missing
                        .into_iter()
                        .map(|p| Step::KeptWorktree(p, why.clone())),
                );
            }
        }
    }
    // Branches: only `backlot/<studio>/*`, and only once their
    // worktrees are out of the way.
    let pattern = format!("{}{}", branch_prefix(studio), "*");
    let branches = git(
        &[
            "-C",
            repo,
            "branch",
            "--list",
            "--format=%(refname:short)",
            &pattern,
        ],
        repo,
    );
    if branches.code != 0 {
        return Err(Fail::refused(format!(
            "studio '{studio}': git branch --list: {}",
            err_line(&branches)
        )));
    }
    let prefix = branch_prefix(studio);
    for branch in branches
        .stdout
        .lines()
        .map(str::trim)
        .filter(|b| !b.is_empty() && b.starts_with(&prefix))
    {
        if dry_run {
            if kept_branches.contains(branch) {
                steps.push(Step::KeptBranch(branch.to_string(), "worktree kept".into()));
                continue;
            }
            let merged = git(
                &["-C", repo, "merge-base", "--is-ancestor", branch, "HEAD"],
                repo,
            )
            .code
                == 0;
            if merged || force {
                steps.push(Step::DeletedBranch(branch.to_string()));
            } else {
                steps.push(Step::KeptBranch(branch.to_string(), "not merged".into()));
            }
            continue;
        }
        let flag = if force { "-D" } else { "-d" };
        let out = git(&["-C", repo, "branch", flag, branch], repo);
        if out.code == 0 {
            steps.push(Step::DeletedBranch(branch.to_string()));
        } else {
            steps.push(Step::KeptBranch(branch.to_string(), err_line(&out)));
        }
    }
    Ok(steps)
}

/// The human-readable plan/outcome for one studio.
fn render(studio: &str, finished: bool, steps: &[Step], dry_run: bool) -> String {
    let mut out = String::new();
    let state = if finished { "finished" } else { "live" };
    if dry_run {
        let _ = writeln!(out, "dry run: studio {studio} ({state}): prune");
    } else {
        let _ = writeln!(out, "studio {studio} ({state}): prune");
    }
    if steps.is_empty() {
        let _ = writeln!(out, "  nothing to prune");
        return out;
    }
    for step in steps {
        match step {
            Step::RemovedWorktree(p) => {
                let verb = if dry_run { "would remove" } else { "removed" };
                let _ = writeln!(out, "  {verb} worktree {p}");
            }
            Step::PrunedWorktree(p) => {
                let verb = if dry_run { "would prune" } else { "pruned" };
                let _ = writeln!(out, "  {verb} missing worktree {p}");
            }
            Step::KeptWorktree(p, why) => {
                let _ = writeln!(out, "  kept worktree {p} — {why}");
            }
            Step::DeletedBranch(b) => {
                let verb = if dry_run { "would delete" } else { "deleted" };
                let _ = writeln!(out, "  {verb} branch {b}");
            }
            Step::KeptBranch(b, why) => {
                let _ = writeln!(out, "  kept branch {b} — {why}");
            }
        }
    }
    out
}

/// `backlot studio prune [<studio> | --all-finished] [--dry-run] [--force]`.
pub(crate) fn run(
    base: &Path,
    studio: Option<String>,
    all_finished: bool,
    dry_run: bool,
    force: bool,
) -> Result<String, Fail> {
    let names = match (&studio, all_finished) {
        (Some(name), false) => vec![crate::cli::check_studio_name(name)?],
        (None, true) => store::studios(base).map_err(|e| Fail::refused(e.to_string()))?,
        _ => return Err(Fail::usage("give a studio or --all-finished, not both")),
    };
    if names.is_empty() {
        return Ok("no finished studios to prune\n".into());
    }
    let mut git = real_git;
    let mut blocks = Vec::new();
    for name in names {
        let dir = store::studio_dir(base, &name);
        if !dir.join("events.jsonl").is_file() {
            if all_finished {
                continue;
            }
            return Err(Fail::refused(format!("no studio '{name}'")));
        }
        let state = store::load_state(&dir).map_err(crate::hwood::store_fail)?;
        if state.finished.is_none() {
            // The one hard rule: a live studio's worktrees are in use.
            if all_finished {
                continue;
            }
            return Err(Fail::usage(format!(
                "studio '{name}' is not finished; prune never touches a live studio"
            )));
        }
        let Some(cfg) = &state.studio else {
            return Err(Fail::refused(format!("studio '{name}' has no events")));
        };
        let repo = cfg.repo.clone();
        let probe = git(&["-C", &repo, "rev-parse", "--is-inside-work-tree"], &repo);
        if probe.code != 0 || probe.stdout.trim() != "true" {
            return Err(Fail::refused(format!(
                "studio '{name}': {repo} is not a git work tree"
            )));
        }
        let steps = prune_studio(&mut git, &name, &dir, &repo, dry_run, force)?;
        blocks.push(render(&name, true, &steps, dry_run));
    }
    if blocks.is_empty() {
        return Ok("no finished studios to prune\n".into());
    }
    Ok(blocks.join("\n"))
}

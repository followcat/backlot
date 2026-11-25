//! `backlot studio prune`: reap a finished studio's worktrees and
//! `backlot/<studio>/*` branches. The ledger, `reports/`, `messages/`
//! and `attention/` are never touched — `backlot records` and
//! `backlot status` keep the full history.
//!
//! Which repositories hold those trees follows the binding (D4, D10):
//! a single-repository studio binds a work tree, a workspace binds a
//! parent directory whose member repositories hold the per-member
//! `work/integration/<member>` trees, and a no-git studio has neither
//! — it prunes with no git step at all rather than refusing.
//!
//! Safety first: a live studio is refused, a clean worktree goes with
//! `git worktree remove`, a dirty one is kept unless `--force`, and a
//! branch is deleted with `git branch -d` (never `-D` without
//! `--force`) so unmerged integration work is never lost. `--dry-run`
//! reads git and prints the plan without changing anything.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use backlot_model::state::StudioConfig;

use crate::cli::Fail;
use crate::driver::{LocalDriver, MachineDriver};
use crate::prune_paths::{shown, under};
use crate::store;
use crate::workareas::{GitRun, err_line};

/// One thing prune did — or, in a dry run, would do.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Step {
    RemovedWorktree(String),
    PrunedWorktree(String),
    KeptWorktree(String, String),
    DeletedBranch(String),
    KeptBranch(String, String),
    /// A recorded member repository git could not look at — kept
    /// while the other members still run.
    KeptRepo(String, String),
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

/// The `backlot/<studio>/` branch prefix — the only branches prune may
/// ever touch.
fn branch_prefix(studio: &str) -> String {
    format!("backlot/{studio}/")
}

/// What the binding says prune may work, and what it must report
/// instead. Members come from the ledger's recorded list — the same
/// list `workareas::ensure` lays the trees out from — never a scan of
/// the parent directory, so a repository added or moved after the
/// binding is not mistaken for one of this studio's.
///
/// - no-git: no repositories at all; the pass prunes its state only.
/// - workspace: every recorded member is probed on its own. One that
///   is not a work tree today is reported kept and the others still
///   run — one broken member never strands the studio's other trees.
/// - single repository, not a work tree: the one hard refusal, since
///   the studio's whole layout lived there.
pub(crate) enum Plan {
    /// The repositories to work, in recorded order.
    Repos(Vec<String>),
    /// A single-repository binding whose repo is not a work tree.
    NotWorkTree,
}

pub(crate) fn plan(git: &mut GitRun<'_>, cfg: &StudioConfig) -> (Plan, Vec<Step>) {
    if cfg.no_git {
        return (Plan::Repos(Vec::new()), Vec::new());
    }
    let is_work_tree = |git: &mut GitRun<'_>, repo: &str| {
        let probe = git(&["-C", repo, "rev-parse", "--is-inside-work-tree"], repo);
        probe.code == 0 && probe.stdout.trim() == "true"
    };
    if cfg.members.is_empty() {
        return match is_work_tree(git, &cfg.repo) {
            true => (Plan::Repos(vec![cfg.repo.clone()]), Vec::new()),
            false => (Plan::NotWorkTree, Vec::new()),
        };
    }
    let mut repos = Vec::new();
    let mut steps = Vec::new();
    for member in &cfg.members {
        if is_work_tree(git, member) {
            repos.push(member.clone());
        } else {
            steps.push(Step::KeptRepo(
                member_leaf(member),
                "not a git work tree".into(),
            ));
        }
    }
    (Plan::Repos(repos), steps)
}

/// A member repository's leaf name — how the output names it, the
/// same leaf the per-member worktree directory carries.
pub(crate) fn member_leaf(member: &str) -> String {
    Path::new(member)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| member.to_string())
}

/// Work the finished studio's `work/` trees first, then its branches.
/// `git` is injected so the planning tests stay fast and pure; the
/// caller passes [`real_git`] in production. `branch_label` names the
/// repository in branch lines when a workspace has several — the
/// branch name alone cannot say which member lost it.
pub(crate) fn prune_studio(
    git: &mut GitRun<'_>,
    studio: &str,
    dir: &Path,
    repo: &str,
    branch_label: Option<&str>,
    dry_run: bool,
    force: bool,
) -> Result<Vec<Step>, Fail> {
    let list = git(&["-C", repo, "worktree", "list", "--porcelain"], repo);
    if list.code != 0 {
        return Err(Fail::refused(format!(
            "studio '{studio}': {repo}: git worktree list: {}",
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
    // A workspace's members all share one branch name, so each
    // member's lines carry it, `<member>: backlot/<studio>/…`; a
    // single repository keeps the bare name.
    let name = |b: &str| match branch_label {
        Some(leaf) => format!("{leaf}: {b}"),
        None => b.to_string(),
    };
    for branch in branches
        .stdout
        .lines()
        .map(str::trim)
        .filter(|b| !b.is_empty() && b.starts_with(&prefix))
    {
        if dry_run {
            if kept_branches.contains(branch) {
                steps.push(Step::KeptBranch(name(branch), "worktree kept".into()));
                continue;
            }
            let merged = git(
                &["-C", repo, "merge-base", "--is-ancestor", branch, "HEAD"],
                repo,
            )
            .code
                == 0;
            if merged || force {
                steps.push(Step::DeletedBranch(name(branch)));
            } else {
                steps.push(Step::KeptBranch(name(branch), "not merged".into()));
            }
            continue;
        }
        let flag = if force { "-D" } else { "-d" };
        let out = git(&["-C", repo, "branch", flag, branch], repo);
        if out.code == 0 {
            steps.push(Step::DeletedBranch(name(branch)));
        } else {
            steps.push(Step::KeptBranch(name(branch), err_line(&out)));
        }
    }
    Ok(steps)
}

/// `backlot studio prune [<studio> | --all-finished] [--dry-run] [--force]`.
pub(crate) fn run(
    base: &Path,
    studio: Option<String>,
    all_finished: bool,
    dry_run: bool,
    force: bool,
) -> Result<String, Fail> {
    run_with(
        &LocalDriver::new(),
        base,
        studio,
        all_finished,
        dry_run,
        force,
    )
}

/// [`run`] through a driver's git runner.
pub(crate) fn run_with(
    driver: &dyn MachineDriver,
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
    let mut git = driver.git();
    let git = &mut **git;
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
        // Which repositories hold this studio's trees. A no-git
        // binding has none and a broken member is reported kept, so
        // either way the pass still reaches the state records; only
        // a single-repository binding with no work tree refuses.
        let (plan, mut steps) = plan(git, cfg);
        let repos = match plan {
            Plan::NotWorkTree => {
                return Err(Fail::refused(format!(
                    "studio '{name}': {} is not a git work tree",
                    cfg.repo
                )));
            }
            Plan::Repos(repos) => repos,
        };
        for repo in repos {
            // A workspace names the member in its branch lines; a
            // single repository needs no such qualifier.
            let label = (!cfg.members.is_empty()).then(|| member_leaf(&repo));
            match prune_studio(git, &name, &dir, &repo, label.as_deref(), dry_run, force) {
                Ok(more) => steps.extend(more),
                // A member that passes its probe but then refuses a
                // git step is kept as a whole and the others still
                // run: one member's trouble never strands the studio.
                // A single repository keeps its failure, as before.
                Err(e) if cfg.members.is_empty() => return Err(e),
                Err(e) => steps.push(Step::KeptRepo(member_leaf(&repo), e.message().to_string())),
            }
        }
        blocks.push(crate::prune_render::render(&name, true, &steps, dry_run));
    }
    if blocks.is_empty() {
        return Ok("no finished studios to prune\n".into());
    }
    Ok(blocks.join("\n"))
}

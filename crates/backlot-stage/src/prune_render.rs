//! The human-readable plan/outcome block `studio prune` prints.

use std::fmt::Write as _;

use crate::prune::Step;

/// One studio's block: the header (a dry run says so), then every
/// step in the order it ran, or `nothing to prune`.
pub(crate) fn render(studio: &str, finished: bool, steps: &[Step], dry_run: bool) -> String {
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
            Step::KeptRepo(repo, why) => {
                let _ = writeln!(out, "  kept {repo} — {why}");
            }
        }
    }
    out
}

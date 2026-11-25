//! The human-readable plan/outcome block `studio prune` prints.

use std::fmt::Write as _;

use crate::cli::Fail;
use crate::prune::{PruneReport, Step};

/// The whole report: pretty JSON for `--json`, else one block per
/// studio (`no finished studios to prune` when there are none).
pub(crate) fn format_report(report: &PruneReport, json: bool) -> Result<String, Fail> {
    if json {
        return serde_json::to_string_pretty(report)
            .map_err(|e| Fail::refused(format!("studio prune: {e}")));
    }
    if report.studios.is_empty() {
        return Ok("no finished studios to prune\n".into());
    }
    Ok(report
        .studios
        .iter()
        .map(|s| render(&s.studio, s.finished, &s.steps, s.dry_run))
        .collect::<Vec<_>>()
        .join("\n"))
}

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

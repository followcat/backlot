//! Human-readable and JSON rendering for `backlot studio delete`.

use std::fmt::Write as _;

use crate::cli::Fail;
use crate::delete::DeleteReport;

pub(crate) fn format_report(report: &DeleteReport, json: bool) -> Result<String, Fail> {
    if json {
        return serde_json::to_string_pretty(report)
            .map_err(|e| Fail::refused(format!("studio delete: {e}")));
    }
    Ok(render(report).trim_end().to_string())
}

pub(crate) fn render(r: &DeleteReport) -> String {
    let mut out = String::new();
    if r.dry_run {
        let _ = writeln!(out, "dry run: studio {}: delete", r.studio);
    } else if r.archived {
        let _ = writeln!(out, "studio {}: deleted", r.studio);
    } else {
        let _ = writeln!(
            out,
            "studio {}: delete blocked (kept items require --force)",
            r.studio
        );
    }
    if r.finished {
        let verb = if r.dry_run {
            "would finish"
        } else {
            "finished"
        };
        let _ = writeln!(out, "  {verb} studio (reason: deleted)");
    }
    if r.closed_terminals.is_empty() {
        let _ = writeln!(out, "  terminals: none");
    } else {
        let verb = if r.dry_run { "would close" } else { "closed" };
        let _ = writeln!(
            out,
            "  {verb} {} terminal(s): {}",
            r.closed_terminals.len(),
            r.closed_terminals.join(", ")
        );
    }
    for p in &r.removed_worktrees {
        let verb = if r.dry_run { "would remove" } else { "removed" };
        let _ = writeln!(out, "  {verb} worktree {p}");
    }
    for p in &r.pruned_worktrees {
        let verb = if r.dry_run { "would prune" } else { "pruned" };
        let _ = writeln!(out, "  {verb} missing worktree {p}");
    }
    for b in &r.deleted_branches {
        let verb = if r.dry_run { "would delete" } else { "deleted" };
        let _ = writeln!(out, "  {verb} branch {b}");
    }
    for k in &r.kept {
        let _ = writeln!(out, "  {k}");
    }
    if r.dry_run {
        let _ = writeln!(out, "  would archive ledger to {}", r.archive);
    } else if r.archived {
        let _ = writeln!(out, "  archived ledger to {}", r.archive);
    } else {
        let _ = writeln!(out, "  ledger not archived (kept items remain)");
    }
    out
}

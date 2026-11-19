//! The sheet's line model: each tab flattens its data into
//! one-line `SheetLine`s — a display text, a style kind, and the
//! studio-relative file `o` opens when the line carries one.
//! Building lives here so the draw pass only paints.

use backlot_model::event::{Cause, Severity, Verdict};
use backlot_model::hollywood::status_str;
use backlot_model::state::StudioState;

use super::lines_ledger::{deliveries, records};
use super::{Sheet, SheetTab};

/// A line's tone — the draw pass maps these to palette colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Row {
    /// Plain content text.
    Text,
    /// A section head — `casts`, `d2`, a review's verdict line.
    Head,
    /// A dim note — `loading…`, `no deliveries yet`, a finding.
    Dim,
    /// A check or verdict that passed.
    Good,
    /// A failing check or a `changes_requested`/`blocked` verdict.
    Bad,
}

/// One flattened content line.
pub(crate) struct SheetLine {
    pub text: String,
    pub row: Row,
    /// The studio-relative file `o` reads — `reports/…`,
    /// `checks/…` — on the rows that carry one.
    pub file: Option<String>,
    /// Whether the selection may rest here — separator rows are
    /// skipped like the dock's dividers (S2 §4).
    pub selectable: bool,
}

pub(super) fn line(text: impl Into<String>, row: Row) -> SheetLine {
    SheetLine {
        text: text.into(),
        row,
        file: None,
        selectable: true,
    }
}

pub(super) fn file_line(text: impl Into<String>, row: Row, file: impl Into<String>) -> SheetLine {
    SheetLine {
        text: text.into(),
        row,
        file: Some(file.into()),
        selectable: true,
    }
}

pub(super) fn sep() -> SheetLine {
    SheetLine {
        text: String::new(),
        row: Row::Dim,
        file: None,
        selectable: false,
    }
}

/// A verdict as one word, the ledger's spelling.
pub(super) fn verdict_str(v: Verdict) -> &'static str {
    match v {
        Verdict::Accepted => "accepted",
        Verdict::PartiallyAccepted => "partially accepted",
        Verdict::ChangesRequested => "changes requested",
        Verdict::Blocked => "blocked",
    }
}

pub(super) fn severity_str(s: Severity) -> &'static str {
    match s {
        Severity::Low => "low",
        Severity::Medium => "medium",
        Severity::High => "high",
    }
}

pub(super) fn cause_str(c: Cause) -> &'static str {
    match c {
        Cause::DeliveryDefect => "delivery",
        Cause::UnresolvedRework => "rework",
        Cause::RegressionFromFix => "regression",
        Cause::RequirementChange => "requirement",
        Cause::UpstreamError => "upstream",
        Cause::Environment => "environment",
        Cause::ResearchNegative => "research",
        Cause::Unattributed => "unattributed",
    }
}

/// The tab's content lines, summary-first: the task projection shows
/// at once, `loading ledger details…` marks the reads still out.
pub(crate) fn lines_for(
    sheet: &Sheet,
    summary: Option<&backlot_model::summary::StudioSummary>,
) -> Vec<SheetLine> {
    let mut lines = Vec::new();
    match sheet.tab {
        SheetTab::Overview => overview(sheet, summary, &mut lines),
        SheetTab::Deliveries => deliveries(sheet, &mut lines),
        SheetTab::Records => records(sheet, &mut lines),
    }
    if sheet.loading() && matches!(sheet.tab, SheetTab::Overview) && sheet.task.is_some() {
        if !lines.is_empty() {
            lines.push(sep());
        }
        lines.push(line("loading ledger details…", Row::Dim));
    }
    if lines.is_empty() {
        lines.push(line("nothing to show", Row::Dim));
    }
    lines
}

/// `1 Overview & Casts`: the task's title/status/acceptance and
/// dependency chain, then every cast entry. With no task selected
/// the studio's own overview — repo, requirement, owed step — and
/// its task list stand in.
fn overview(
    sheet: &Sheet,
    summary: Option<&backlot_model::summary::StudioSummary>,
    out: &mut Vec<SheetLine>,
) {
    if let Some(err) = sheet.status.as_ref().and_then(|s| s.as_ref().err()) {
        out.push(line(format!("status: {err}"), Row::Bad));
        return;
    }
    let state = sheet.status.as_ref().and_then(|s| s.as_ref().ok());
    let Some(task_id) = sheet.task.as_deref() else {
        studio_overview(summary, state, out);
        return;
    };
    task_overview(summary, state, task_id, out);
}

/// The no-task overview: the studio's own header facts, then its
/// task list once the ledger answered.
fn studio_overview(
    summary: Option<&backlot_model::summary::StudioSummary>,
    state: Option<&StudioState>,
    out: &mut Vec<SheetLine>,
) {
    if let Some(studio) = summary {
        out.push(line(format!("{} — {}", studio.id, studio.state), Row::Head));
        if let Some(repo) = &studio.repo {
            out.push(line(format!("repo {repo}"), Row::Text));
        }
        out.push(line(
            format!(
                "r{} · {}/{} done",
                studio.requirement_version, studio.tasks_done, studio.tasks_total
            ),
            Row::Text,
        ));
        // P4a: runner-reported usage — present only while a
        // delivery recorded any of the counters.
        if let Some(usage) = &studio.usage {
            out.push(line(format!("usage {}", usage.compact()), Row::Text));
        }
        if let Some(next) = &studio.next {
            out.push(line(
                format!("→ {}", crate::panel::owed_text(studio, next)),
                Row::Text,
            ));
        }
    }
    let Some(state) = state else {
        out.push(line("loading ledger details…", Row::Dim));
        return;
    };
    out.push(sep());
    out.push(line("tasks", Row::Head));
    for task in state.tasks.values() {
        let mut text = format!(
            "· {} \"{}\" {}",
            task.id,
            task.title,
            status_str(task.status)
        );
        if !task.dependencies.is_empty() {
            text = format!("{text} (waits {})", task.dependencies.join(", "));
        }
        out.push(line(text, Row::Text));
    }
}

/// One task's overview: the summary projection first — the sheet
/// opens instantly — then the ledger's acceptance, chain and casts.
fn task_overview(
    summary: Option<&backlot_model::summary::StudioSummary>,
    state: Option<&StudioState>,
    task_id: &str,
    out: &mut Vec<SheetLine>,
) {
    let head =
        |title: &str, status: &str| line(format!("{task_id} \"{title}\" {status}"), Row::Head);
    if let Some(t) = summary.and_then(|s| s.tasks.iter().find(|t| t.id == task_id)) {
        out.push(head(&t.title, status_str(t.status)));
    }
    let Some(state) = state else {
        return;
    };
    let Some(task) = state.tasks.get(task_id) else {
        out.push(line(format!("no task '{task_id}' in the ledger"), Row::Bad));
        return;
    };
    if summary.is_none() {
        out.push(head(&task.title, status_str(task.status)));
    }
    let mut pin = format!("pin v{}", task.requirement_version);
    if task.is_behind(state.requirement_version) {
        pin = format!("{pin} → v{} · behind", state.requirement_version);
    }
    out.push(line(pin, Row::Text));
    if state.rework_cap() > 0 {
        out.push(line(
            format!("rework {}/{}", task.rework_rounds, state.rework_cap()),
            Row::Text,
        ));
    }
    if !task.acceptance.is_empty() {
        out.push(sep());
        out.push(line("acceptance", Row::Head));
        for l in task.acceptance.lines().take(12) {
            out.push(line(format!("  {l}"), Row::Text));
        }
    }
    if !task.dependencies.is_empty() {
        out.push(sep());
        out.push(line("dependency chain", Row::Head));
        for dep in &task.dependencies {
            let dep_state = state
                .tasks
                .get(dep)
                .map(|d| status_str(d.status))
                .unwrap_or("missing");
            out.push(line(format!("  waits {dep} · {dep_state}"), Row::Text));
        }
    }
    let casts: Vec<&backlot_model::state::CastRecord> = if task.casts.is_empty() {
        task.cast.iter().collect()
    } else {
        task.casts.iter().collect()
    };
    out.push(sep());
    out.push(line("casts", Row::Head));
    if casts.is_empty() {
        out.push(line("  uncast", Row::Dim));
    }
    for cast in casts {
        let actor = &cast.actor;
        let mut text = format!("  {} → {}/{}", cast.candidate, actor.runner, actor.model);
        if let Some(effort) = &actor.effort {
            text = format!("{text}/{effort}");
        }
        if !cast.reason.is_empty() {
            text = format!("{text} — {}", cast.reason);
        }
        if !cast.by.is_empty() {
            text = format!("{text} (by {})", cast.by);
        }
        out.push(line(text, Row::Text));
    }
}

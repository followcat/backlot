//! The sheet's line model: each tab flattens its data into
//! one-line `SheetLine`s — a display text, a style kind, and the
//! studio-relative file `o` opens when the line carries one.
//! Building lives here so the draw pass only paints.

use backlot_model::event::{Cause, Severity, Verdict};
use backlot_model::hollywood::status_str;
use backlot_model::state::{DeliveryRecord, ReviewRecord, StudioState};

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

fn line(text: impl Into<String>, row: Row) -> SheetLine {
    SheetLine {
        text: text.into(),
        row,
        file: None,
        selectable: true,
    }
}

fn file_line(text: impl Into<String>, row: Row, file: impl Into<String>) -> SheetLine {
    SheetLine {
        text: text.into(),
        row,
        file: Some(file.into()),
        selectable: true,
    }
}

fn sep() -> SheetLine {
    SheetLine {
        text: String::new(),
        row: Row::Dim,
        file: None,
        selectable: false,
    }
}

/// A verdict as one word, the ledger's spelling.
fn verdict_str(v: Verdict) -> &'static str {
    match v {
        Verdict::Accepted => "accepted",
        Verdict::PartiallyAccepted => "partially accepted",
        Verdict::ChangesRequested => "changes requested",
        Verdict::Blocked => "blocked",
    }
}

fn severity_str(s: Severity) -> &'static str {
    match s {
        Severity::Low => "low",
        Severity::Medium => "medium",
        Severity::High => "high",
    }
}

fn cause_str(c: Cause) -> &'static str {
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

/// `2 Deliveries & Reviews`: every delivery with its commit range,
/// the actor-reported checks, the stage-run `CheckedRecord`s with
/// exit codes and durations — `o` opens the delivery's report or the
/// check's log — and every review with verdict, conclusion and
/// findings.
fn deliveries(sheet: &Sheet, out: &mut Vec<SheetLine>) {
    let Some(task_id) = sheet.task.as_deref() else {
        out.push(line(
            "open the sheet on a task row to read its deliveries",
            Row::Dim,
        ));
        return;
    };
    match sheet.status.as_ref() {
        None => {
            out.push(line("loading ledger details…", Row::Dim));
            return;
        }
        Some(Err(err)) => {
            out.push(line(format!("status: {err}"), Row::Bad));
            return;
        }
        Some(Ok(_)) => {}
    }
    let state = sheet.status.as_ref().unwrap().as_ref().unwrap();
    let Some(task) = state.tasks.get(task_id) else {
        out.push(line(format!("no task '{task_id}' in the ledger"), Row::Bad));
        return;
    };
    if task.deliveries.is_empty() {
        out.push(line("no deliveries yet", Row::Dim));
    }
    for delivery in &task.deliveries {
        delivery_line(delivery, out);
        for reported in &delivery.checks {
            out.push(line(
                format!(
                    "    {} {} reported {}",
                    if reported.passed { "✓" } else { "×" },
                    reported.name,
                    if reported.passed { "pass" } else { "fail" },
                ),
                if reported.passed { Row::Good } else { Row::Bad },
            ));
        }
        for check in task.checked_of(delivery.n) {
            let exit = check
                .exit_code
                .map(|c| format!("exit {c}"))
                .unwrap_or_else(|| "no exit".to_string());
            let timeout = if check.timed_out { " · timed out" } else { "" };
            out.push(file_line(
                format!(
                    "    {} {} {exit} · {}ms{timeout}",
                    if check.passed() { "✓" } else { "×" },
                    check.name,
                    check.duration_ms
                ),
                if check.passed() { Row::Good } else { Row::Bad },
                check.output.clone(),
            ));
        }
        for review in task.reviews.iter().filter(|r| r.delivery == delivery.n) {
            review_lines(review, out);
        }
        out.push(sep());
    }
    let stray: Vec<&ReviewRecord> = task
        .reviews
        .iter()
        .filter(|r| !task.deliveries.iter().any(|d| d.n == r.delivery))
        .collect();
    if !stray.is_empty() {
        out.push(line("reviews without a delivery", Row::Head));
        for review in stray {
            review_lines(review, out);
        }
    }
}

/// One delivery's head line: `d2 <runner>/<model> · <commits> ·
/// v<req>` — the report file rides the row for `o`.
fn delivery_line(delivery: &DeliveryRecord, out: &mut Vec<SheetLine>) {
    let actor = format!("{}/{}", delivery.actor.runner, delivery.actor.model);
    let commits = if delivery.commits.is_empty() {
        "no commits".to_string()
    } else {
        delivery.commits.clone()
    };
    let text = format!(
        "d{} {} · {} · v{}",
        delivery.n, actor, commits, delivery.requirement_version
    );
    if delivery.report.is_empty() {
        out.push(line(text, Row::Head));
    } else {
        out.push(file_line(text, Row::Head, delivery.report.clone()));
    }
}

/// One review: verdict + conclusion on the head line, findings and
/// upstream changes below with their severity and fault tags.
fn review_lines(review: &ReviewRecord, out: &mut Vec<SheetLine>) {
    let (mark, row) = match review.verdict {
        Verdict::Accepted | Verdict::PartiallyAccepted => ("✓", Row::Good),
        Verdict::ChangesRequested | Verdict::Blocked => ("×", Row::Bad),
    };
    out.push(line(
        format!(
            "  {mark} d{} {} · {} — {}",
            review.delivery,
            review.reviewer,
            verdict_str(review.verdict),
            review.conclusion
        ),
        row,
    ));
    for finding in &review.findings {
        out.push(line(
            format!(
                "    {} [{} · {}] {}",
                finding.id,
                severity_str(finding.severity),
                cause_str(finding.cause),
                finding.summary
            ),
            Row::Dim,
        ));
    }
    for change in &review.upstream_changes {
        out.push(line(
            format!("    ↑ {} — {}", change.summary, change.reason),
            Row::Dim,
        ));
    }
}

/// `3 Actor Records`: one block per actor identity the studio's
/// `records --json` names — factual counts only (D8).
fn records(sheet: &Sheet, out: &mut Vec<SheetLine>) {
    match sheet.records.as_ref() {
        None => {
            out.push(line("loading ledger details…", Row::Dim));
        }
        Some(Err(err)) => {
            out.push(line(format!("records: {err}"), Row::Bad));
        }
        Some(Ok(recs)) => {
            if recs.is_empty() {
                out.push(line("no records for this studio", Row::Dim));
            }
            for rec in recs {
                let actor = rec.get("actor").cloned().unwrap_or_default();
                let name = format!(
                    "{}/{}",
                    actor.get("runner").and_then(|r| r.as_str()).unwrap_or("?"),
                    actor.get("model").and_then(|m| m.as_str()).unwrap_or("?")
                );
                let kind = rec
                    .get("task_kind")
                    .and_then(|k| k.as_str())
                    .unwrap_or("task");
                let tasks = rec.get("tasks").and_then(|t| t.as_u64()).unwrap_or(0);
                let deliveries = rec.get("deliveries").and_then(|d| d.as_u64()).unwrap_or(0);
                out.push(line(format!("{name} · {kind}"), Row::Head));
                out.push(line(
                    format!("  {tasks} tasks · {deliveries} deliveries"),
                    Row::Text,
                ));
                let first = rec.get("first_delivery").cloned().unwrap_or_default();
                let num = |k: &str| first.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
                out.push(line(
                    format!(
                        "  first delivery: {} accepted · {} partial · {} changes · {} blocked · {} unreviewed",
                        num("accepted"),
                        num("partially_accepted"),
                        num("changes_requested"),
                        num("blocked"),
                        num("unreviewed")
                    ),
                    Row::Text,
                ));
                let n = |k: &str| rec.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
                out.push(line(
                    format!(
                        "  defects {} ({} open · {} own · {} repeats) · stage checks {}✓/{}× · reported {}✓/{}×",
                        n("defects"),
                        n("defects_open"),
                        n("actor_fault_defects"),
                        n("defect_repeats"),
                        n("stage_checks_passed"),
                        n("stage_checks_failed"),
                        n("reported_checks_passed"),
                        n("reported_checks_failed"),
                    ),
                    Row::Text,
                ));
                if rec.get("enough_data").and_then(|v| v.as_bool()) == Some(false) {
                    out.push(line("  not enough data", Row::Dim));
                }
                out.push(sep());
            }
        }
    }
}

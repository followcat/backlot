//! The sheet's ledger tabs: `2 Deliveries & Reviews` — every
//! delivery with its short commit range, checks and `o`-openable
//! files — and `3 Actor Records` with each role's counts.

use backlot_model::event::Verdict;
use backlot_model::state::{DeliveryRecord, ReviewRecord};

use super::Sheet;
use super::lines::*;

/// the actor-reported checks, the stage-run `CheckedRecord`s with
/// exit codes and durations — `o` opens the delivery's report or the
/// check's log — and every review with verdict, conclusion and
/// findings.
pub(super) fn deliveries(sheet: &Sheet, out: &mut Vec<SheetLine>) {
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
pub(super) fn records(sheet: &Sheet, out: &mut Vec<SheetLine>) {
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

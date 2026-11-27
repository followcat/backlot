//! Deterministic markdown renderer for `backlot digest`.

use std::fmt::Write as _;

use backlot_model::metrics::{DigestReport, StudioDigest};

fn format_reset(reset_at_ms: Option<u64>, now_ms: u64) -> String {
    match reset_at_ms {
        Some(reset) if reset > now_ms => format!(
            "until {} (in {})",
            crate::ratelimit::format_reset_local(reset, now_ms),
            crate::render::fmt_ms(reset - now_ms)
        ),
        Some(reset) => format!(
            "until {}",
            crate::ratelimit::format_reset_local(reset, now_ms)
        ),
        None => "reset unknown".into(),
    }
}

fn render_studio_digest_md(out: &mut String, s: &StudioDigest, now_ms: u64) {
    let _ = writeln!(out, "## {}", s.studio);
    let finished_tag = if s.studio_finished_ts_ms.is_some() {
        " · studio finished"
    } else {
        ""
    };
    let _ = writeln!(out, "checks: `{}`{finished_tag}\n", s.checks_status);

    let _ = writeln!(out, "### Finished");
    if s.finished.is_empty() {
        let _ = writeln!(out, "- none");
    } else {
        for f in &s.finished {
            let _ = writeln!(out, "- `{}`: {} ({})", f.task, f.title, f.status);
        }
    }

    let _ = writeln!(out, "\n### Delivered");
    if s.delivered.is_empty() {
        let _ = writeln!(out, "- none");
    } else {
        for d in &s.delivered {
            let commits = d.commits.as_deref().unwrap_or("-");
            let _ = writeln!(
                out,
                "- `{}` d{}: `{commits}` — {}",
                d.task, d.delivery, d.title
            );
        }
    }

    let _ = writeln!(out, "\n### Sent back (changes_requested)");
    if s.sent_back.is_empty() {
        let _ = writeln!(out, "- none");
    } else {
        for r in &s.sent_back {
            let note = r.note.as_deref().unwrap_or("-");
            let _ = writeln!(
                out,
                "- `{}` d{} (by {}): {note}",
                r.task, r.delivery, r.reviewer
            );
        }
    }

    let _ = writeln!(out, "\n### Blocked");
    if s.blocked.is_empty() {
        let _ = writeln!(out, "- none");
    } else {
        for b in &s.blocked {
            let reason = b.reason.as_deref().unwrap_or("-");
            if b.waits_for.is_empty() {
                let _ = writeln!(out, "- `{}`: {} ({reason})", b.task, b.title);
            } else {
                let _ = writeln!(
                    out,
                    "- `{}`: {} ({reason}; waits on {})",
                    b.task,
                    b.title,
                    b.waits_for.join(", ")
                );
            }
        }
    }

    let _ = writeln!(out, "\n### Waiting on person");
    if s.waiting_on_user.attentions.is_empty()
        && s.waiting_on_user.asks.is_empty()
        && !s.waiting_on_user.design_waiting
    {
        let _ = writeln!(out, "- none");
    } else {
        for a in &s.waiting_on_user.attentions {
            let _ = writeln!(out, "- attention `{}`: {} ({})", a.role, a.reason, a.detail);
        }
        for q in &s.waiting_on_user.asks {
            let _ = writeln!(out, "- ask from `{}` (#{})", q.role, q.seq);
        }
        if s.waiting_on_user.design_waiting {
            let _ = writeln!(out, "- design gate waiting for person");
        }
    }

    let _ = writeln!(out, "\n### Rate limits");
    let _ = writeln!(
        out,
        "- window events: {}, clusters: {}, max/window: {}",
        s.rate_limits.events_in_window,
        s.rate_limits.windows_in_window,
        s.rate_limits.max_per_window
    );
    for r in &s.rate_limits.open {
        let reset = format_reset(r.reset_at_ms, now_ms);
        let _ = writeln!(out, "- open on `{}` ({reset})", r.role);
    }

    let _ = writeln!(out, "\n### Coverage matrix");
    if s.coverage.is_empty() {
        let _ = writeln!(out, "- no tasks recorded");
    } else {
        let _ = writeln!(
            out,
            "| design section | task | commits | task.checked | verdict |"
        );
        let _ = writeln!(out, "| --- | --- | --- | --- | --- |");
        for row in &s.coverage {
            let sec = row.design_section.as_deref().unwrap_or("-");
            let commits = row.commits.as_deref().unwrap_or("-");
            let _ = writeln!(
                out,
                "| {sec} | {} | {commits} | {} | {} |",
                row.task, row.checks_summary, row.verdict
            );
        }
    }
    out.push('\n');
}

/// Render `DigestReport` in deterministic markdown.
pub(crate) fn render_digest_md(report: &DigestReport, window: &str) -> String {
    let mut out = format!("# backlot digest (since: {window})\n");
    if report.unknown_events_skipped > 0 {
        let _ = writeln!(
            out,
            "\n_unknown events skipped: {}_",
            report.unknown_events_skipped
        );
    }
    out.push('\n');
    if report.studios.is_empty() {
        out.push_str("No studios recorded.\n");
        return out;
    }
    for s in &report.studios {
        render_studio_digest_md(&mut out, s, report.now_ms);
    }
    out
}

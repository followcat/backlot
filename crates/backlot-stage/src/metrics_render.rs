//! Deterministic <=100-column table renderer for `backlot metrics`.

use std::fmt::Write as _;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use backlot_model::metrics::{AggregateMetrics, AttentionMetrics, MetricsReport, StudioMetrics};

const MAX_COLS: usize = 100;

struct QualityCols<'a> {
    name: &'a str,
    deliv: usize,
    pass_rate: Option<f64>,
    cr_rate: Option<f64>,
    early: usize,
    chk_col: &'a str,
    chk_pass: usize,
    chk_fail: usize,
    findings: usize,
    tells: Option<f64>,
}

struct GuardsCols<'a> {
    name: &'a str,
    allow: usize,
    deny: usize,
    ref_tot: usize,
    ref_no209: usize,
    hw_drop: usize,
    hw_sent: usize,
    open_ms: Option<u64>,
    rev_p50: Option<u64>,
    rev_p90: Option<u64>,
    wall_p50: Option<u64>,
    pause_c: usize,
    pause_t: usize,
}

pub(crate) fn trunc_width(s: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(s) <= max_width {
        return s.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return s
            .chars()
            .next()
            .filter(|c| c.width().unwrap_or(0) <= 1)
            .map(|_| "…".to_string())
            .unwrap_or_default();
    }
    let budget = max_width - 1;
    let mut out = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        if w + cw > budget {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

fn pad_left_col(s: &str, width: usize) -> String {
    let fitted = trunc_width(s, width);
    let pad = width.saturating_sub(UnicodeWidthStr::width(fitted.as_str()));
    format!("{fitted}{}", " ".repeat(pad))
}

fn push_line(out: &mut String, line: &str) {
    let clean = trunc_width(line.trim_end(), MAX_COLS);
    out.push_str(&clean);
    out.push('\n');
}

fn fmt_pct(r: Option<f64>) -> String {
    match r {
        Some(v) => format!("{:.0}%", v * 100.0),
        None => "-".into(),
    }
}

fn fmt_ratio(r: Option<f64>) -> String {
    match r {
        Some(v) => format!("{v:.2}"),
        None => "-".into(),
    }
}

fn fmt_dur_ms(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1_000.0)
    } else if ms < 3_600_000 {
        format!("{:.1}m", ms as f64 / 60_000.0)
    } else if ms < 86_400_000 {
        format!("{:.1}h", ms as f64 / 3_600_000.0)
    } else {
        format!("{:.1}d", ms as f64 / 86_400_000.0)
    }
}

fn fmt_opt_dur(ms: Option<u64>) -> String {
    ms.map_or_else(|| "-".into(), fmt_dur_ms)
}

fn fmt_count_dur(count: usize, ms: u64) -> String {
    if count == 0 {
        "0".into()
    } else {
        format!("{count}/{}", fmt_dur_ms(ms))
    }
}

fn wrap_items(prefix: &str, items: &[String], out: &mut String) {
    if items.is_empty() {
        return;
    }
    let mut cur = prefix.to_string();
    let indent_w = UnicodeWidthStr::width(prefix);
    let indent = " ".repeat(indent_w);
    for (idx, item) in items.iter().enumerate() {
        let sep = if cur == prefix { "" } else { ", " };
        let next_w = UnicodeWidthStr::width(cur.as_str())
            + UnicodeWidthStr::width(sep)
            + UnicodeWidthStr::width(item.as_str());
        if idx > 0 && next_w > MAX_COLS && cur != prefix {
            push_line(out, &cur);
            cur = format!("{indent}{item}");
        } else {
            cur.push_str(sep);
            cur.push_str(item);
        }
    }
    push_line(out, &cur);
}

fn push_quality_row(out: &mut String, r: QualityCols<'_>) {
    let line = format!(
        "{}  {:>5}  {:>8}  {:>7}  {:>5}  {:>7}  {:>9}  {:>5}  {:>6}",
        pad_left_col(r.name, 16),
        r.deliv,
        fmt_pct(r.pass_rate),
        fmt_pct(r.cr_rate),
        r.early,
        r.chk_col,
        format!("{}/{}", r.chk_pass, r.chk_fail),
        r.findings,
        fmt_ratio(r.tells)
    );
    push_line(out, &line);
}

fn push_attn_row(out: &mut String, name: &str, attn: &AttentionMetrics) {
    let line = format!(
        "{}  {:>7}  {:>11}  {:>11}  {:>11}  {:>11}  {:>13}",
        pad_left_col(name, 16),
        format!("{}({})", attn.total, attn.open),
        fmt_count_dur(
            attn.by_class.needs_you.count,
            attn.by_class.needs_you.open_ms
        ),
        fmt_count_dur(attn.by_class.stuck.count, attn.by_class.stuck.open_ms),
        fmt_count_dur(attn.by_class.info.count, attn.by_class.info.open_ms),
        fmt_count_dur(
            attn.permission_prompts_not_needs_user.count,
            attn.permission_prompts_not_needs_user.open_ms
        ),
        format!(
            "{}/{}/{}",
            attn.rate_limited.total, attn.rate_limited.windows, attn.rate_limited.max_per_window
        ),
    );
    push_line(out, &line);
}

fn push_guards_row(out: &mut String, r: GuardsCols<'_>) {
    let line = format!(
        "{}  {:>8}  {:>10}  {:>9}  {:>6}  {:>13}  {:>8}  {:>7}",
        pad_left_col(r.name, 16),
        format!("{}/{}", r.allow, r.deny),
        format!("{}/{}", r.ref_tot, r.ref_no209),
        format!("{}/{}", r.hw_drop, r.hw_sent),
        fmt_opt_dur(r.open_ms),
        format!("{}/{}", fmt_opt_dur(r.rev_p50), fmt_opt_dur(r.rev_p90)),
        fmt_opt_dur(r.wall_p50),
        format!("{}/{}", r.pause_c, r.pause_t),
    );
    push_line(out, &line);
}

fn render_studio_quality(out: &mut String, s: &StudioMetrics) {
    let chk_col = if s.checks.configured {
        format!("{} decl", s.checks.configured_checks)
    } else {
        "none".into()
    };
    push_quality_row(
        out,
        QualityCols {
            name: &s.studio,
            deliv: s.quality.delivered_tasks,
            pass_rate: s.quality.first_delivery_pass_rate,
            cr_rate: s.quality.changes_requested_rate,
            early: s.quality.early_tasks,
            chk_col: &chk_col,
            chk_pass: s.checks.task_checked_passed,
            chk_fail: s.checks.task_checked_failed,
            findings: s.findings.findings,
            tells: s.continuity.tells_per_task,
        },
    );
}

fn render_all_quality(out: &mut String, a: &AggregateMetrics) {
    let cov = format!("{}/{}", a.studios_with_checks, a.studios_total);
    push_quality_row(
        out,
        QualityCols {
            name: "ALL",
            deliv: a.quality.delivered_tasks,
            pass_rate: a.quality.first_delivery_pass_rate,
            cr_rate: a.quality.changes_requested_rate,
            early: a.quality.early_tasks,
            chk_col: &cov,
            chk_pass: a.checks.task_checked_passed,
            chk_fail: a.checks.task_checked_failed,
            findings: a.findings.findings,
            tells: a.continuity.tells_per_task,
        },
    );
}

/// Render `MetricsReport` as compact tables strictly fitting in `<= 100` columns.
pub(crate) fn render_metrics_table(report: &MetricsReport, since: Option<&str>) -> String {
    let mut out = String::new();
    let window = since.unwrap_or("all");
    let mut hdr = format!(
        "backlot metrics (since: {window}, studios: {})",
        report.studios.len()
    );
    if report.unknown_events_skipped > 0 {
        let _ = write!(hdr, " [unknown skipped: {}]", report.unknown_events_skipped);
    }
    push_line(&mut out, &hdr);
    if report.studios.is_empty() {
        push_line(&mut out, "(no studios recorded)");
        return out;
    }

    push_line(&mut out, "");
    push_line(
        &mut out,
        "studio            deliv  1st-pass  CR-rate  early   checks  chk(p/f)   find  tell/t",
    );
    for s in &report.studios {
        render_studio_quality(&mut out, s);
    }
    render_all_quality(&mut out, &report.all);

    push_line(&mut out, "");
    push_line(
        &mut out,
        "studio            attn(o)     need-you        stuck         info     pp(!usr)  rl(ev/win/max)",
    );
    for s in &report.studios {
        push_attn_row(&mut out, &s.studio, &s.attention);
    }
    push_attn_row(&mut out, "ALL", &report.all.attention);

    push_line(&mut out, "");
    push_line(
        &mut out,
        "studio            appr(a/d)  ref(t/-209)  hw(d/s)   open    rev(p50/p90)  wall-p50  pause(c/t)",
    );
    for s in &report.studios {
        push_guards_row(
            &mut out,
            GuardsCols {
                name: &s.studio,
                allow: s.approvals.allow,
                deny: s.approvals.deny,
                ref_tot: s.refused.total,
                ref_no209: s.refused.without_probes,
                hw_drop: s.hollywood.notices_dropped,
                hw_sent: s.hollywood.notices_sent,
                open_ms: s.timing.opening_ms,
                rev_p50: s.timing.review_latency_p50_ms,
                rev_p90: s.timing.review_latency_p90_ms,
                wall_p50: s.timing.task_wall_clock_p50_ms,
                pause_c: s.timing.pause_clusters,
                pause_t: s.timing.paused_tasks_in_clusters,
            },
        );
    }
    let a = &report.all;
    push_guards_row(
        &mut out,
        GuardsCols {
            name: "ALL",
            allow: a.approvals.allow,
            deny: a.approvals.deny,
            ref_tot: a.refused.total,
            ref_no209: a.refused.without_probes,
            hw_drop: a.hollywood.notices_dropped,
            hw_sent: a.hollywood.notices_sent,
            open_ms: a.timing.opening_p50_ms,
            rev_p50: a.timing.review_latency_p50_ms,
            rev_p90: a.timing.review_latency_p90_ms,
            wall_p50: a.timing.task_wall_clock_p50_ms,
            pause_c: a.timing.pause_clusters,
            pause_t: a.timing.paused_tasks_in_clusters,
        },
    );

    if !report.all.refused.by_rule.is_empty() {
        let items: Vec<String> = report
            .all
            .refused
            .by_rule
            .iter()
            .map(|(r, c)| format!("{r} {}/{}", c.total, c.without_probes))
            .collect();
        wrap_items("refused by rule (total/-209): ", &items, &mut out);
    }
    out
}

pub(crate) use crate::metrics_digest_render::render_digest_md;

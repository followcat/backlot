//! Plain-text formatting, ask/answer pairing, and safe file-reading helpers
//! for `backlot msg show`.

use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use backlot_model::design::DesignQuestion;
use backlot_model::event::{Cause, Event, EventKind, Finding, Severity};
use backlot_model::state::StudioState;

use crate::cli::Fail;
use crate::msg_show::{MsgAnswerView, MsgView};

const MAX_BYTES: u64 = 1024 * 1024;

/// Format milliseconds as UTC timestamp or `"unknown"` when `0`.
pub(crate) fn format_when(ts_ms: u64) -> String {
    if ts_ms == 0 {
        "unknown".into()
    } else {
        crate::design_doc::utc_timestamp(ts_ms)
    }
}

/// Match a user `tell` against the newest preceding unanswered `ask` from `role`.
pub(crate) fn paired_ask_for_tell(events: &[Event], target_idx: usize, role: &str) -> Option<u64> {
    for ev in events[..target_idx].iter().rev() {
        match &ev.kind {
            EventKind::AskAnswered { role: r, .. } | EventKind::AskAcknowledged { role: r, .. }
                if r == role =>
            {
                return None;
            }
            EventKind::MessageQueued {
                to,
                purpose,
                source_seq,
                ..
            } => {
                if ev.by.role == "user" && to == role && purpose.is_none() && source_seq.is_none() {
                    return None;
                }
                if ev.by.role == role && to == "user" && purpose.as_deref() == Some("ask") {
                    return Some(ev.seq);
                }
            }
            _ => {}
        }
    }
    None
}

/// Find the resolving answer (`tell` or terminal `AskAnswered`/`AskAcknowledged`)
/// for the `ask` event at `ask_idx`.
pub(crate) fn find_answer_for_ask(
    dir: &Path,
    events: &[Event],
    state: &StudioState,
    ask_idx: usize,
    asker: &str,
) -> Result<Option<MsgAnswerView>, Fail> {
    let ask_seq = events[ask_idx].seq;
    for ev in &events[ask_idx + 1..] {
        match &ev.kind {
            EventKind::MessageQueued {
                to,
                file,
                purpose,
                source_seq,
            } => {
                if ev.by.role == asker && to == "user" && purpose.as_deref() == Some("ask") {
                    return Ok(None);
                }
                let rec = state.messages.iter().find(|m| m.seq == ev.seq);
                let eff_to = rec.map_or(to.as_str(), |m| m.to.as_str());
                let eff_file = rec.map_or_else(|| file.clone(), |m| m.file.clone());
                if ev.by.role == "user"
                    && eff_to == asker
                    && purpose.is_none()
                    && source_seq.is_none()
                {
                    let body = match &eff_file {
                        Some(rel) => read_studio_file(dir, rel)?,
                        None => {
                            return Err(Fail::refused(format!("message #{} has no body", ev.seq)));
                        }
                    };
                    return Ok(Some(MsgAnswerView {
                        seq: ev.seq,
                        kind: "answer".into(),
                        from: "user".into(),
                        to: eff_to.to_string(),
                        ts_ms: ev.ts_ms,
                        when: format_when(ev.ts_ms),
                        file: eff_file,
                        body,
                    }));
                }
            }
            EventKind::AskAnswered { role, target } if *target == ask_seq => {
                return Ok(Some(MsgAnswerView {
                    seq: ev.seq,
                    kind: "answer".into(),
                    from: "user".into(),
                    to: role.clone(),
                    ts_ms: ev.ts_ms,
                    when: format_when(ev.ts_ms),
                    file: None,
                    body: format!("Answered ask #{ask_seq} directly in {role}'s terminal."),
                }));
            }
            EventKind::AskAcknowledged { role, target } if *target == ask_seq => {
                return Ok(Some(MsgAnswerView {
                    seq: ev.seq,
                    kind: "acknowledged".into(),
                    from: "user".into(),
                    to: role.clone(),
                    ts_ms: ev.ts_ms,
                    when: format_when(ev.ts_ms),
                    file: None,
                    body: format!("Acknowledged ask #{ask_seq} without text reply."),
                }));
            }
            _ => {}
        }
    }
    Ok(None)
}

/// Validate `raw` as a studio-relative path strictly contained inside `dir`
/// (rejecting `..`, NUL, absolute paths, and escaping symlinks) and read up to 1 MiB.
pub(crate) fn read_studio_file(dir: &Path, raw: &str) -> Result<String, Fail> {
    let file = Path::new(raw);
    if raw.is_empty() || raw.contains('\0') || file.is_absolute() {
        return Err(Fail::refused(format!(
            "path '{raw}' must be a studio-relative path"
        )));
    }
    let mut rel = PathBuf::new();
    for c in file.components() {
        match c {
            Component::Normal(p) => rel.push(p),
            _ => {
                return Err(Fail::refused(format!("path '{raw}' must not contain '..'")));
            }
        }
    }
    let root = dir
        .canonicalize()
        .map_err(|e| Fail::refused(e.to_string()))?;
    let real = dir
        .join(&rel)
        .canonicalize()
        .map_err(|_| Fail::refused(format!("no file '{}'", rel.display())))?;
    if !real.starts_with(&root) || !real.is_file() {
        return Err(Fail::refused(format!(
            "'{}' is outside studio ledger",
            rel.display()
        )));
    }
    let bytes = real
        .metadata()
        .map_err(|e| Fail::refused(e.to_string()))?
        .len();
    if bytes > MAX_BYTES {
        return Err(Fail::refused(format!(
            "'{}' is {bytes} bytes — the msg show limit is {MAX_BYTES} bytes",
            rel.display()
        )));
    }
    let content = std::fs::read(&real).map_err(|e| Fail::refused(e.to_string()))?;
    Ok(String::from_utf8_lossy(&content).into_owned())
}

/// Append structured summary questions to a design entry body.
pub(crate) fn append_questions(body: &mut String, questions: &[DesignQuestion]) {
    if questions.is_empty() {
        return;
    }
    if !body.ends_with('\n') && !body.is_empty() {
        body.push('\n');
    }
    let _ = writeln!(body, "\nQuestions:");
    for q in questions {
        let sec = q
            .section_ref
            .as_deref()
            .map(|s| format!(", ref: {s}"))
            .unwrap_or_default();
        let _ = writeln!(
            body,
            "- [{}] {} (recommendation: {}{sec})",
            q.id, q.prompt, q.recommendation
        );
    }
}

/// Format task findings inline for `backlot msg show`.
pub(crate) fn format_findings(task: &str, delivery: u32, findings: &[Finding]) -> String {
    let mut out = format!("Task {task} delivery {delivery} findings:\n");
    for f in findings {
        let sev = match f.severity {
            Severity::Low => "low",
            Severity::Medium => "medium",
            Severity::High => "high",
        };
        let cause = match f.cause {
            Cause::DeliveryDefect => "delivery_defect",
            Cause::UnresolvedRework => "unresolved_rework",
            Cause::RegressionFromFix => "regression_from_fix",
            Cause::RequirementChange => "requirement_change",
            Cause::UpstreamError => "upstream_error",
            Cause::Environment => "environment",
            Cause::ResearchNegative => "research_negative",
            Cause::Unattributed => "unattributed",
        };
        let _ = writeln!(out, "- [{}] ({sev}, {cause}): {}", f.id, f.summary);
    }
    out
}

/// Render one studio message in plain text for humans and role terminals.
pub(crate) fn render_text(view: &MsgView) -> String {
    let mut out = format!(
        "#{} · {} · {} -> {} · {}\n",
        view.seq, view.kind, view.from, view.to, view.when
    );
    if let Some(rb) = &view.recorded_by {
        let _ = writeln!(out, "Recorded by: {rb}");
    }
    if let Some(src) = &view.source {
        let _ = writeln!(out, "Source kind: {src}");
    }
    if let Some(node) = &view.node {
        let _ = writeln!(out, "Node: {node}");
    }
    if let Some(file) = &view.file {
        let _ = writeln!(out, "File: {file}");
    }
    if let Some(src_seq) = view.source_seq {
        let _ = writeln!(out, "Source: #{src_seq}");
    }
    if let Some(reply_to) = view.in_reply_to {
        let _ = writeln!(out, "In reply to: #{reply_to}");
    }
    if !view.refs.is_empty() {
        let refs = view
            .refs
            .iter()
            .map(|r| format!("#{r}"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "Refs: {refs}");
    }
    let body = view.body.trim_end_matches('\n');
    if !body.is_empty() {
        out.push('\n');
        out.push_str(body);
        out.push('\n');
    }
    if let Some(ans) = &view.answer {
        let _ = writeln!(
            out,
            "\n--- Answer (#{} · {} · {} · {}) ---",
            ans.seq, ans.kind, ans.from, ans.when
        );
        if let Some(file) = &ans.file {
            let _ = writeln!(out, "File: {file}");
        }
        let ans_body = ans.body.trim_end_matches('\n');
        if !ans_body.is_empty() {
            out.push('\n');
            out.push_str(ans_body);
            out.push('\n');
        }
    }
    out
}

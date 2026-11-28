//! `backlot msg show <seq> [--json]`: read-only lookup of one studio
//! message, ask (with its answer if present), design note, supplement,
//! sign-off, report, delivery report, or finding from the caller's own studio ledger.

use std::path::Path;

use backlot_model::design::{DesignSlot, EntrySource, SignMode};
use backlot_model::event::{Event, EventKind};
use backlot_model::state::StudioState;
use serde::Serialize;

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::msg_show_fmt::{
    append_questions, find_answer_for_ask, format_findings, format_when, paired_ask_for_tell,
    read_studio_file, render_text,
};
use crate::store::{self, StoreError};

/// Answer attached to an `ask` message when resolved by a reply `tell`
/// or `ask.answered` / `ask.acknowledged`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MsgAnswerView {
    pub seq: u64,
    pub kind: String,
    pub from: String,
    pub to: String,
    pub ts_ms: u64,
    pub when: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    pub body: String,
}

/// Structured view rendered by `backlot msg show <seq> [--json]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MsgView {
    pub studio: String,
    pub seq: u64,
    pub kind: String,
    pub from: String,
    /// The coding agent that recorded it for the person, if any — the
    /// `from` line then reads `user (via claude-code)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    pub to: String,
    pub ts_ms: u64,
    pub when: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_reply_to: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dropped: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<MsgAnswerView>,
    pub body: String,
}

/// Read one studio message by ledger `seq` from the caller's own studio.
pub(crate) fn show(
    common: &Common,
    env: &Env,
    base: &Path,
    seq: u64,
    json: bool,
) -> Result<String, Fail> {
    crate::cli::check_identity(common, env)?;
    let studio = crate::cli::studio_name(common, env)?;
    let resolved = crate::cli::resolve_base(env, common, base, &studio)?;
    let dir = store::studio_dir(&resolved, &studio);
    crate::verbs::require_studio_dir(&dir, &studio)?;

    let (events, _) = store::load_events(&dir).map_err(store_err)?;
    let state = store::load_state(&dir).map_err(store_err)?;
    let Some(pos) = events.iter().position(|e| e.seq == seq) else {
        return Err(Fail::refused(missing_seq_msg(&studio, seq, &events)));
    };

    let view = build_view(&studio, &dir, &events, pos, &state)?;
    if json {
        return serde_json::to_string_pretty(&view)
            .map_err(|e| Fail::refused(format!("msg show json: {e}")));
    }
    Ok(render_text(&view))
}

fn missing_seq_msg(studio: &str, seq: u64, events: &[Event]) -> String {
    match events.last().map(|e| e.seq) {
        Some(last) => format!("no event #{seq} in studio '{studio}' (last seq is {last})"),
        None => format!("no event #{seq} in studio '{studio}' (ledger is empty)"),
    }
}

fn store_err(e: StoreError<()>) -> Fail {
    Fail::refused(match e {
        StoreError::Io(err) => err.to_string(),
        StoreError::Corrupt(c) => c.to_string(),
        StoreError::Refused(r) => r.to_string(),
        StoreError::Build(()) => "failed to load studio state".into(),
    })
}

fn build_view(
    studio: &str,
    dir: &Path,
    events: &[Event],
    pos: usize,
    state: &StudioState,
) -> Result<MsgView, Fail> {
    let ev = &events[pos];
    let mut view = MsgView {
        studio: studio.to_string(),
        seq: ev.seq,
        kind: String::new(),
        from: ev.by.label(),
        via: ev
            .by
            .via
            .as_deref()
            .and_then(|v| v.strip_prefix("agent:"))
            .map(str::to_string),
        to: String::new(),
        ts_ms: ev.ts_ms,
        when: format_when(ev.ts_ms),
        file: None,
        purpose: None,
        source_seq: None,
        in_reply_to: None,
        recorded_by: None,
        source: None,
        node: None,
        refs: Vec::new(),
        delivered: None,
        dropped: None,
        answer: None,
        body: String::new(),
    };

    match &ev.kind {
        EventKind::MessageQueued {
            to,
            file,
            purpose,
            source_seq,
        } => populate_queued(
            dir,
            events,
            pos,
            state,
            to,
            file.as_deref(),
            purpose.as_deref(),
            *source_seq,
            &mut view,
        )?,
        EventKind::AskAnswered { role, target } => {
            view.kind = "answer".into();
            view.from = "user".into();
            view.via = None;
            view.to = role.clone();
            view.in_reply_to = Some(*target);
            view.recorded_by = (ev.by.role != "user").then(|| ev.by.role.clone());
            view.body = format!("Answered ask #{target} directly in {role}'s terminal.");
        }
        EventKind::AskAcknowledged { role, target } => {
            view.kind = "acknowledged".into();
            view.from = "user".into();
            view.via = None;
            view.to = role.clone();
            view.in_reply_to = Some(*target);
            view.body = format!("Acknowledged ask #{target} without text reply.");
        }
        EventKind::DesignEntry {
            slot,
            file,
            source,
            refs,
            questions,
            ..
        } => {
            view.kind = design_slot_kind(*slot).into();
            if *slot == DesignSlot::UserInput {
                view.from = "user".into();
                view.via = None;
                view.recorded_by = (ev.by.role != "user").then(|| ev.by.role.clone());
            }
            view.to = "design".into();
            view.file = Some(file.clone());
            view.source = source.map(entry_source_name).map(str::to_string);
            view.refs = refs.clone();
            let mut body = read_studio_file(dir, file)?;
            append_questions(&mut body, questions);
            view.body = body;
        }
        EventKind::DesignSigned {
            version,
            round,
            mode,
            doc_file,
            note,
        } => {
            view.kind = "design sign-off".into();
            view.to = "studio".into();
            let mode_str = match mode {
                SignMode::Approved => "approved",
                SignMode::Skipped => "skipped",
            };
            let rel_doc = (!doc_file.is_empty())
                .then_some(doc_file.as_str())
                .filter(|rel| dir.join(rel).is_file());
            view.file = rel_doc.map(str::to_string);
            let mut body = if let Some(rel) = rel_doc {
                read_studio_file(dir, rel)?
            } else {
                format!("Design v{version} r{round} signed ({mode_str}).")
            };
            if let Some(n) = note.as_deref().filter(|s| !s.is_empty())
                && !body.contains(n)
            {
                if !body.ends_with('\n') && !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(&format!("Note: {n}\n"));
            }
            view.body = body;
        }
        EventKind::ReportSent { to, node, file } => {
            let rec = state.reports.iter().find(|r| r.seq == ev.seq);
            let eff_to = rec.map_or(to.as_str(), |r| r.to.as_str()).to_string();
            let eff_node = rec.map_or(node.as_str(), |r| r.node.as_str()).to_string();
            let eff_file = rec.map_or_else(|| file.clone(), |r| r.file.clone());
            view.kind = "report".into();
            view.to = eff_to;
            view.node = Some(eff_node);
            view.file = Some(eff_file.clone());
            view.delivered = rec.map(|r| r.delivered);
            view.body = read_studio_file(dir, &eff_file)?;
        }
        EventKind::TaskFinding {
            task,
            delivery,
            findings,
        } => {
            view.kind = "finding".into();
            view.to = task.clone();
            view.body = format_findings(task, *delivery, findings);
        }
        EventKind::TaskDelivered { task, report, .. } => {
            view.kind = "delivery report".into();
            view.to = state
                .tasks
                .get(task)
                .map_or_else(|| "director".to_string(), |t| t.reviewer().to_string());
            view.file = Some(report.clone());
            view.body = read_studio_file(dir, report)?;
        }
        other => {
            return Err(Fail::refused(format!(
                "event #{} is '{}', not a message",
                ev.seq,
                other.raw_name()
            )));
        }
    }

    Ok(view)
}

#[allow(clippy::too_many_arguments)]
fn populate_queued(
    dir: &Path,
    events: &[Event],
    pos: usize,
    state: &StudioState,
    to: &str,
    file: Option<&str>,
    purpose: Option<&str>,
    source_seq: Option<u64>,
    view: &mut MsgView,
) -> Result<(), Fail> {
    let ev = &events[pos];
    let rec = state.messages.iter().find(|m| m.seq == ev.seq);
    let eff_to = rec.map_or(to, |m| m.to.as_str()).to_string();
    let eff_file = rec.map_or_else(|| file.map(str::to_string), |m| m.file.clone());
    let in_reply_to = if purpose.is_none() && source_seq.is_none() && ev.by.role == "user" {
        paired_ask_for_tell(events, pos, &eff_to)
    } else {
        None
    };
    view.kind = message_kind(purpose, source_seq, in_reply_to);
    view.to = eff_to.clone();
    view.file = eff_file.clone();
    view.purpose = purpose.map(str::to_string);
    view.source_seq = source_seq;
    view.in_reply_to = in_reply_to;
    view.delivered = rec.map(|m| m.delivered);
    view.dropped = rec.map(|m| m.dropped);
    view.body = match &eff_file {
        Some(rel) => read_studio_file(dir, rel)?,
        None if purpose == Some("plan") => format!("(plan request to {eff_to})"),
        None => return Err(Fail::refused(format!("message #{} has no body", ev.seq))),
    };
    if purpose == Some("ask") {
        view.answer = find_answer_for_ask(dir, events, state, pos, &ev.by.role)?;
    }
    Ok(())
}

fn message_kind(
    purpose: Option<&str>,
    source_seq: Option<u64>,
    in_reply_to: Option<u64>,
) -> String {
    match purpose {
        Some("ask") => "ask".into(),
        Some("plan") => "plan request".into(),
        Some("dispatch") => "dispatch".into(),
        Some("nudge") => "nudge".into(),
        Some(other) => other.into(),
        None if source_seq.is_some() => "notice".into(),
        None if in_reply_to.is_some() => "answer".into(),
        None => "tell".into(),
    }
}

fn design_slot_kind(slot: DesignSlot) -> &'static str {
    match slot {
        DesignSlot::UserInput => "design note",
        DesignSlot::Supplement => "supplement",
        DesignSlot::Proposal => "design proposal",
        DesignSlot::Buildability => "design buildability",
        DesignSlot::Summary => "design summary",
    }
}

fn entry_source_name(src: EntrySource) -> &'static str {
    match src {
        EntrySource::Tell => "tell",
        EntrySource::Hook => "hook",
        EntrySource::ProducerNote => "producer_note",
    }
}

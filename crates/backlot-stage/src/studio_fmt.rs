//! The studio lifecycle verbs' one-line results — the text the local
//! CLI prints, and the text a remote ack is rendered into from the
//! same struct.

use backlot_model::req_results::{StudioFinishResult, StudioNewResult, StudioReopenResult};

/// The first non-empty line of a message, trimmed and capped at the
/// 60 characters `studio new --dry-run` restates.
pub(crate) fn first_message_line(body: &str) -> String {
    let line = body
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or_default()
        .trim();
    line.chars().take(60).collect()
}

pub(crate) fn studio_new(r: &StudioNewResult) -> String {
    if r.dry_run {
        let mut out = format!(
            "dry run: studio '{}' ('{}', repo {}) is valid — nothing recorded",
            r.studio, r.title, r.repo
        );
        if let Some(line) = &r.first_message {
            out.push_str(&format!("\nfirst message to the producer: {line}"));
        }
        return out;
    }
    let seq = r.seq.unwrap_or(0);
    let mut out = format!("event {seq}: studio.created '{}'", r.title);
    if let Some(ask) = r.ask_seq {
        out.push_str(&format!("\nevent {ask}: message.queued → producer"));
    }
    out
}

pub(crate) fn studio_finish(r: &StudioFinishResult) -> String {
    format!("event {}: studio.finished", r.seq)
}

pub(crate) fn studio_reopen(r: &StudioReopenResult) -> String {
    format!("event {}: studio.reopened", r.seq)
}

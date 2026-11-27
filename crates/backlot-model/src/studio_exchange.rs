//! User-facing studio conversation projection (`StudioSummary.exchange`):
//! merges messages involving `user` (plus plain `hollywood → producer`
//! tells) and `to: user` reports on their studio ledger `seq`, capped
//! to the newest [`MAX_STUDIO_EXCHANGE`] turns at [`HEADLINE_CHARS`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::hollywood_summary::HEADLINE_CHARS;
use crate::state::{Message, StudioState, actor_task};
use crate::summary::cap;

/// Maximum user-facing exchange turns carried per studio in the summary.
pub const MAX_STUDIO_EXCHANGE: usize = 6;

/// One user-facing turn in a studio's conversation timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StudioExchange {
    /// Studio ledger `seq` — orders messages and reports on one timeline.
    pub seq: u64,
    /// Sender role (`user`, `producer`, `hollywood`, …).
    pub who: String,
    /// Recipient role (`user`, `producer`, `hollywood`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// Turn kind (`"tell"` for `message.queued`, `"report"` for `report.sent`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// True when this tell carries the explicit person-question mark.
    #[serde(default, skip_serializing_if = "crate::event::is_false")]
    pub ask: bool,
    /// Task id (`t1`, …) for task-bound reports and actor messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// Ledger timestamp in milliseconds, omitted when `0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts_ms: Option<u64>,
    /// One-line snippet (≤ [`HEADLINE_CHARS`] = 200 chars).
    pub text: String,
    /// Studio-relative path (`messages/…` or `reports/…`) opened on Enter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Reason recorded by `message.dropped`, omitted for live/delivered turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped_reason: Option<String>,
}

enum RawTurn<'a> {
    Message(&'a Message),
    Report(&'a crate::state::ReportRecord),
}

impl RawTurn<'_> {
    fn seq(&self) -> u64 {
        match self {
            RawTurn::Message(m) => m.seq,
            RawTurn::Report(r) => r.seq,
        }
    }
}

/// Whether a studio `message.queued` belongs on the user-facing dock
/// timeline: excludes dropped notices, stage-relayed notices
/// (`source_seq.is_some()`), and workflow envelopes (`purpose.is_some()`).
fn is_user_facing_message(m: &Message) -> bool {
    m.source_seq.is_none()
        && (m.purpose.is_none() || matches!(m.purpose.as_deref(), Some("ask") | Some("decision")))
        && (m.from == "user" || m.to == "user" || m.from == "hollywood")
}

fn newest_turns(state: &StudioState) -> Vec<RawTurn<'_>> {
    let mut turns: Vec<RawTurn<'_>> = state
        .messages
        .iter()
        .filter(|m| is_user_facing_message(m))
        .map(RawTurn::Message)
        .chain(
            state
                .reports
                .iter()
                .filter(|r| r.to == "user")
                .map(RawTurn::Report),
        )
        .collect();
    turns.sort_by_key(RawTurn::seq);
    turns.split_off(turns.len().saturating_sub(MAX_STUDIO_EXCHANGE))
}

/// `(seq, relpath)` pairs for the newest [`MAX_STUDIO_EXCHANGE`]
/// user-facing turns that reference an on-disk studio file — lets the
/// stage read at most [`MAX_STUDIO_EXCHANGE`] snippet files per studio.
pub fn studio_exchange_files(state: &StudioState) -> Vec<(u64, String)> {
    newest_turns(state)
        .into_iter()
        .filter_map(|turn| match turn {
            RawTurn::Message(m) => m.file.as_ref().map(|f| (m.seq, f.clone())),
            RawTurn::Report(r) => Some((r.seq, r.file.clone())),
        })
        .collect()
}

fn cap_snippet(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(HEADLINE_CHARS)
        .collect()
}

fn clean_relpath(s: &str) -> Option<String> {
    (!s.is_empty() && s.chars().count() <= HEADLINE_CHARS && !s.chars().any(char::is_control))
        .then(|| s.to_owned())
}

/// Project `state`'s newest [`MAX_STUDIO_EXCHANGE`] user-facing turns
/// using stage-supplied `seq -> snippet` lines (`snippets`). Pure: performs
/// zero filesystem I/O.
pub fn studio_exchange(
    state: &StudioState,
    snippets: &BTreeMap<u64, String>,
) -> Vec<StudioExchange> {
    newest_turns(state)
        .into_iter()
        .map(|turn| match turn {
            RawTurn::Message(m) => {
                let text = snippets
                    .get(&m.seq)
                    .map(|s| cap_snippet(s.trim()))
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| cap(&format!("{} → {}", m.from, m.to)));
                let task = actor_task(&m.from).or_else(|| actor_task(&m.to)).map(cap);
                StudioExchange {
                    seq: m.seq,
                    who: cap(&m.from),
                    to: Some(cap(&m.to)),
                    kind: Some("tell".into()),
                    ask: m.purpose.as_deref() == Some("ask"),
                    task,
                    ts_ms: (m.ts_ms > 0).then_some(m.ts_ms),
                    text,
                    file: m.file.as_deref().and_then(clean_relpath),
                    dropped_reason: m
                        .dropped_reason
                        .clone()
                        .or_else(|| m.dropped.then(|| "dropped".into())),
                }
            }
            RawTurn::Report(r) => {
                let text = snippets
                    .get(&r.seq)
                    .map(|s| cap_snippet(s.trim()))
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| cap(&format!("report: {}", r.node)));
                let task = if state.tasks.contains_key(&r.node) {
                    Some(cap(&r.node))
                } else {
                    actor_task(&r.from).map(cap)
                };
                StudioExchange {
                    seq: r.seq,
                    who: cap(&r.from),
                    to: Some(cap(&r.to)),
                    kind: Some("report".into()),
                    ask: false,
                    task,
                    ts_ms: (r.ts_ms > 0).then_some(r.ts_ms),
                    text,
                    file: clean_relpath(&r.file),
                    dropped_reason: None,
                }
            }
        })
        .collect()
}

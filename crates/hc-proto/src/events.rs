// Derived from herdr-core schema/hc.openrpc.json and schema/README.md (Apache-2.0).

//! `events.subscribe` and the notifications that follow: `event`, `heartbeat`
//! and `missed`. Event kinds and per-kind `data` stay tolerant — a newer kind
//! or field must not break the inventory.

use crate::jsonrpc::Notification;
use crate::terminal::Terminal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Notification method names after a subscription.
pub mod notification {
    /// One terminal or document fact changed; `params.seq` orders it.
    pub const EVENT: &str = "event";
    /// Sent about every 15 s with the current `seq` when nothing is queued.
    pub const HEARTBEAT: &str = "heartbeat";
    /// The subscriber fell behind; `params` is a fresh snapshot.
    pub const MISSED: &str = "missed";
}

/// The `type` strings an `event` notification can carry in generation 1.
/// Unknown types parse into [`EventParams::kind`] untouched.
pub mod event_kind {
    /// The whole `Terminal` record arrived; add it.
    pub const CREATED: &str = "created";
    /// The program was lost; kept terminals may restart.
    pub const LOST: &str = "lost";
    /// A kept terminal's new shell: the whole record, again.
    pub const RESTARTED: &str = "restarted";
    /// The `keep` flag changed.
    pub const KEEP: &str = "keep";
    /// The program ended for good.
    pub const FINISHED: &str = "finished";
    /// The terminal went away; drop it.
    pub const REMOVED: &str = "removed";
    /// The 7501 status changed: `{status, records, reported}`.
    pub const STATUS: &str = "status";
    /// Complete origin repository replacement: `{repository}` (nullable).
    pub const REPOSITORY: &str = "repository";
    /// Complete origin clock replacement: `{activity}` (nullable).
    pub const ACTIVITY: &str = "activity";
    /// A turn ended: `{turn, state}`.
    pub const TURN: &str = "turn";
    /// The title changed: `{title}`.
    pub const TITLE: &str = "title";
    /// Labels changed: `{labels}` plus `record` when newly matching.
    pub const LABELS: &str = "labels";
    /// The grid size changed: `{cols, rows}`.
    pub const SIZE: &str = "size";
    /// The foreground process changed, or is now null.
    pub const FOREGROUND: &str = "foreground";
    /// A document changed: `{name, version}`; refetch it with `doc.get`.
    pub const DOC: &str = "doc";

    /// The kinds generation 1 documents. Every one must carry a `data`
    /// member — `null` still counts (`foreground` uses it); only omission
    /// is malformed. A kind not listed may omit `data` entirely.
    pub fn requires_data(kind: &str) -> bool {
        matches!(
            kind,
            CREATED
                | LOST
                | RESTARTED
                | KEEP
                | FINISHED
                | REMOVED
                | STATUS
                | REPOSITORY
                | ACTIVITY
                | TURN
                | TITLE
                | LABELS
                | SIZE
                | FOREGROUND
                | DOC
        )
    }
}

/// Params for `events.subscribe`. All absent: every terminal on every added
/// machine, plus document events.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SubscribeParams {
    /// Only this terminal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    /// Only children of this terminal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children_of: Option<String>,
    /// Only terminals carrying all of these labels.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

/// The `events.subscribe` result: a snapshot, then the event stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscribeResult {
    /// Events after this sequence follow. After a `missed` snapshot, ignore
    /// events whose seq is not above the snapshot's.
    pub seq: u64,
    /// Matching terminals here and on connected machines (`machine/t7`).
    /// Snapshots contain terminals only, never documents.
    pub terminals: Vec<Terminal>,
    /// Framed mode: the channel events arrive on. Plain mode: none, and this
    /// connection then carries only notifications.
    #[serde(default)]
    pub channel: Option<u32>,
}

/// The `params` of an `event` notification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventParams {
    /// The event's sequence number; apply only when above the snapshot's.
    pub seq: u64,
    /// What changed; see [`event_kind`]. Kept as a string so newer kinds
    /// parse instead of failing.
    #[serde(rename = "type")]
    pub kind: String,
    /// The terminal it is about, when it is about one (`host-b/t7` on another
    /// machine). Document events carry none.
    #[serde(default)]
    pub terminal: Option<String>,
    /// The machine it comes from, when the core names one (document events
    /// on an added machine do).
    #[serde(default)]
    pub machine: Option<String>,
    /// The change itself; the shape depends on `kind` and stays opaque so
    /// newer kinds and fields pass through. Documented kinds must carry
    /// the member — `null` still counts (`foreground` uses it) — and
    /// [`ServerNotification::from_notification`] rejects its absence.
    #[serde(default)]
    pub data: Value,
}

/// The `params` of a `heartbeat` notification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeartbeatParams {
    /// The current sequence; no event was lost when it matches ours.
    pub seq: u64,
}

/// The `params` of a `missed` notification: a fresh snapshot. Drop events
/// whose seq is not above it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MissedParams {
    /// The snapshot's sequence.
    pub seq: u64,
    /// The new terminal set. Required: a `missed` without it is malformed,
    /// never an empty inventory.
    pub terminals: Vec<Terminal>,
    /// Set when the core reports a remote stream's `missed` locally.
    #[serde(default)]
    pub machine: Option<String>,
}

/// A notification after `events.subscribe`, decoded by method name.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerNotification {
    /// `event`: one fact changed.
    Event(EventParams),
    /// `heartbeat`: nothing queued; carries the current seq.
    Heartbeat(HeartbeatParams),
    /// `missed`: a fresh snapshot replacing events that were dropped.
    Missed(MissedParams),
}

impl ServerNotification {
    /// Decodes a notification as one of the subscription methods. `Ok(None)`
    /// for methods this protocol version does not define — they are ignored,
    /// not errors.
    pub fn from_notification(
        notification: &Notification,
    ) -> Result<Option<ServerNotification>, serde_json::Error> {
        let params = || notification.params.clone().unwrap_or(Value::Null);
        match notification.method.as_str() {
            notification::EVENT => {
                let params = params();
                // A documented kind must carry `data` — present-but-null
                // is a real payload (`foreground`), while a missing member
                // is malformed wire data, not an empty one. Unknown kinds
                // may omit `data` and stay tolerated.
                if let Some(kind) = params.get("type").and_then(Value::as_str)
                    && event_kind::requires_data(kind)
                    && params.get("data").is_none()
                {
                    return Err(<serde_json::Error as serde::de::Error>::custom(format!(
                        "`{kind}` event without its `data` member"
                    )));
                }
                serde_json::from_value(params).map(|p| Some(ServerNotification::Event(p)))
            }
            notification::HEARTBEAT => {
                serde_json::from_value(params()).map(|p| Some(ServerNotification::Heartbeat(p)))
            }
            notification::MISSED => {
                serde_json::from_value(params()).map(|p| Some(ServerNotification::Missed(p)))
            }
            _ => Ok(None),
        }
    }
}

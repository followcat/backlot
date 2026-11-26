//! Forward-compatible deserialization of unknown ledger event kinds.

use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::event::{Event, EventBy, EventKind, default_ledger_version};

/// Wire `kind` tags understood by this build of `backlot-model`.
pub const KNOWN_EVENT_KINDS: &[&str] = &[
    "studio.created",
    "studio.cast",
    "role.started",
    "role.lost",
    "role.resumed",
    "role.closed",
    "role.session",
    "role.recast",
    "role.reopen",
    "role.release",
    "role.restart",
    "req.version",
    "plan.published",
    "task.created",
    "task.cast",
    "task.priority",
    "task.delivered",
    "task.reviewed",
    "task.checked",
    "task.finding",
    "task.state",
    "message.queued",
    "message.delivered",
    "message.dropped",
    "report.sent",
    "attention.raised",
    "attention.escalated",
    "attention.cleared",
    "stage.answered",
    "ask.answered",
    "ask.acknowledged",
    "role.refused",
    "outward.requested",
    "outward.completed",
    "role.progress",
    "role.approval",
    "studio.finished",
    "studio.reopened",
    "studio.deleted",
    "design.opened",
    "design.entry",
    "design.signed",
    "correction",
];

#[derive(Deserialize)]
struct EventHeader {
    #[serde(default = "default_ledger_version")]
    v: u32,
    seq: u64,
    ts_ms: u64,
    by: EventBy,
    kind: String,
}

impl EventKind {
    /// Returns `true` when `tag` matches an event kind known to this build.
    pub fn is_known_tag(tag: &str) -> bool {
        KNOWN_EVENT_KINDS.contains(&tag)
    }
}

impl<'de> Deserialize<'de> for Event {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let val = Value::deserialize(deserializer)?;
        let header: EventHeader =
            serde_json::from_value(val.clone()).map_err(serde::de::Error::custom)?;
        let kind = if EventKind::is_known_tag(&header.kind) {
            serde_json::from_value::<EventKind>(val).map_err(serde::de::Error::custom)?
        } else {
            EventKind::Unknown {
                raw_kind: header.kind,
            }
        };
        Ok(Event {
            v: header.v,
            seq: header.seq,
            ts_ms: header.ts_ms,
            by: header.by,
            kind,
        })
    }
}

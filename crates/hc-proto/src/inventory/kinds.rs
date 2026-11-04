// Derived from herdr-core crates/herdr-core/src/remote.rs (apply, mirror_event)
// and schema/README.md (Apache-2.0).

//! Per-kind event validation and application. Every kind this generation
//! documents is checked *atomically* before a mutation is made — including
//! the presence of documented members that are nullable, where `Option`
//! alone would silently accept omission — so a malformed known event can
//! never mutate a record or be advanced past as if it applied. Unknown
//! kinds are always accepted: they advance the sequence untouched.
//!
//! Whole-record events (`created`/`restarted`, a `labels` `record`) may
//! never key the map differently from the record's own `address`: a
//! `terminal` member disagreeing with it rejects the whole event.

use super::{Effect, Inventory};
use crate::events::{EventParams, event_kind};
use crate::terminal::{Foreground, Status, Terminal};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// `lost`/`finished` data: all four documented members must be present —
/// each nullable, none omissible.
#[derive(Deserialize)]
pub(super) struct EndData {
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    signal: Option<i32>,
    #[serde(default)]
    finished_ms: Option<u64>,
}

/// `status` data: the three documented members must be present — `status`
/// nullable, `records` a map, `reported` a bool.
#[derive(Deserialize)]
pub(super) struct StatusData {
    #[serde(default)]
    status: Option<Status>,
    records: BTreeMap<String, Status>,
    reported: bool,
}

fn malformed(what: &'static str) -> Result<(), &'static str> {
    Err(what)
}

/// The whole-record validation `created`/`restarted` (and a `labels`
/// `record`) share: the payload must be a `Terminal`, and when the event
/// names a `terminal` it must be the record's own address.
fn whole_record(event: &EventParams, data: &Value) -> Result<Terminal, &'static str> {
    let record: Terminal =
        serde_json::from_value(data.clone()).map_err(|_| "whole record does not fit Terminal")?;
    if let Some(terminal) = &event.terminal
        && *terminal != record.address
    {
        return Err("event terminal disagrees with the record's address");
    }
    Ok(record)
}

/// Atomic pre-apply validation for every kind this generation documents.
/// `Err` marks the event malformed: the caller reports a resync and does
/// not advance `seq`. Unknown kinds are always `Ok` — retained, tolerated.
pub(super) fn check(event: &EventParams) -> Result<(), &'static str> {
    let data = &event.data;
    let terminal = || event.terminal.is_some();
    let has = |key: &str| data.get(key).is_some();
    match event.kind.as_str() {
        event_kind::DOC => {
            if data.get("name").and_then(Value::as_str).is_none() {
                return malformed("doc event without a name");
            }
            Ok(())
        }
        event_kind::CREATED | event_kind::RESTARTED => whole_record(event, data).map(|_| ()),
        event_kind::REMOVED => {
            if !terminal() {
                return malformed("removed without a terminal");
            }
            Ok(())
        }
        kind @ (event_kind::LOST | event_kind::FINISHED) => {
            if !terminal() {
                return malformed("end event without a terminal");
            }
            for key in ["reason", "exit_code", "signal", "finished_ms"] {
                if !has(key) {
                    return malformed("end data misses a documented member");
                }
            }
            if serde_json::from_value::<EndData>(data.clone()).is_err() {
                return malformed(if kind == event_kind::LOST {
                    "malformed lost data"
                } else {
                    "malformed finished data"
                });
            }
            Ok(())
        }
        event_kind::STATUS => {
            if !terminal() {
                return malformed("status event without a terminal");
            }
            for key in ["status", "records", "reported"] {
                if !has(key) {
                    return malformed("status data misses a documented member");
                }
            }
            if serde_json::from_value::<StatusData>(data.clone()).is_err() {
                return malformed("malformed status data");
            }
            Ok(())
        }
        event_kind::TURN => {
            if !terminal() || data.get("turn").and_then(Value::as_u64).is_none() {
                return malformed("turn event without a terminal or a turn");
            }
            Ok(())
        }
        event_kind::TITLE => {
            if !terminal() || data.get("title").and_then(Value::as_str).is_none() {
                return malformed("title event without a terminal or a title");
            }
            Ok(())
        }
        event_kind::KEEP => {
            if !terminal() || data.get("keep").and_then(Value::as_bool).is_none() {
                return malformed("keep event without a terminal or a flag");
            }
            Ok(())
        }
        event_kind::SIZE => {
            let cell = |key| {
                data.get(key)
                    .and_then(Value::as_u64)
                    .and_then(|n| u16::try_from(n).ok())
            };
            if !terminal() || cell("cols").is_none() || cell("rows").is_none() {
                return malformed("size event without a terminal or a grid");
            }
            Ok(())
        }
        event_kind::FOREGROUND => {
            if !terminal() || serde_json::from_value::<Option<Foreground>>(data.clone()).is_err() {
                return malformed("foreground event without a terminal or a value");
            }
            Ok(())
        }
        event_kind::LABELS => {
            if !terminal() {
                return malformed("labels event without a terminal");
            }
            let labels_ok = data
                .get("labels")
                .and_then(|v| serde_json::from_value::<BTreeMap<String, String>>(v.clone()).ok())
                .is_some();
            if !labels_ok {
                return malformed("labels event without its label map");
            }
            match data.get("record") {
                None => Ok(()),
                Some(record) => whole_record(event, record).map(|_| ()),
            }
        }
        // Unknown kinds are retained and advance the sequence untouched.
        _ => Ok(()),
    }
}

impl Inventory {
    /// The per-kind dispatch, after `seq` advanced and `check` passed —
    /// the shapes below are all validated (see `apply` in core's remote.rs
    /// for the record mutations).
    pub(super) fn apply_event(&mut self, event: &EventParams, effect: &mut Effect) {
        let data = &event.data;
        if event.kind == event_kind::DOC {
            // No terminal on a doc event; `machine` names the remote core.
            let name = data.get("name").and_then(Value::as_str).expect("checked");
            effect.docs.push(super::DocRef {
                machine: event.machine.clone(),
                name: name.into(),
            });
            return;
        }
        match event.kind.as_str() {
            kind @ (event_kind::CREATED | event_kind::RESTARTED) => {
                let record = whole_record(event, data).expect("checked");
                let address = record.address.clone();
                self.terminals.insert(address.clone(), record);
                if kind == event_kind::CREATED {
                    effect.created.push(address);
                } else {
                    effect.restarted.push(address);
                }
            }
            event_kind::REMOVED => {
                let address = event.terminal.as_ref().expect("checked");
                if self.terminals.remove(address).is_some() {
                    effect.removed.push(address.clone());
                }
            }
            _ => {
                let Some(address) = event.terminal.clone() else {
                    return; // unknown kind without an address: advance only
                };
                match self.terminals.get_mut(&address) {
                    Some(record) => {
                        if event.kind == event_kind::LABELS {
                            self.labels(address, event, effect);
                        } else if apply_partial(record, &event.kind, data) {
                            effect.changed.push(address.clone());
                            if event.kind == event_kind::SIZE {
                                effect.resized.push(address.clone());
                            }
                        }
                    }
                    // Outside the view — but a `labels` event may carry
                    // the whole `record` when it newly matched the filter.
                    None if event.kind == event_kind::LABELS => {
                        self.labels(address, event, effect);
                    }
                    None => {}
                }
            }
        }
    }

    /// `labels` data is `{labels}` plus `record` (the whole `Terminal`)
    /// when the terminal newly matched the subscription's filter — both
    /// already validated. Labels replace the record's; a `record` inserts
    /// wholesale. Under a label filter, a record that stops matching
    /// leaves the view — the event still arrives, and the inventory drops
    /// the record.
    fn labels(&mut self, address: String, event: &EventParams, effect: &mut Effect) {
        let mut in_view = self.terminals.contains_key(&address);
        if in_view {
            let labels = serde_json::from_value(event.data["labels"].clone()).expect("checked");
            let record = self.terminals.get_mut(&address).expect("checked");
            record.labels = labels;
            effect.changed.push(address.clone());
        }
        if let Some(value) = event.data.get("record") {
            let record: Terminal = serde_json::from_value(value.clone()).expect("checked");
            if in_view {
                effect.changed.push(address.clone());
            } else {
                effect.created.push(address.clone());
            }
            self.terminals.insert(address.clone(), record);
            in_view = true;
        }
        if in_view
            && let Some(filter) = &self.label_filter
            && let Some(record) = self.terminals.get(&address)
            && !filter.iter().all(|(k, v)| record.labels.get(k) == Some(v))
        {
            self.terminals.remove(&address);
            effect.removed.push(address);
        }
    }
}

/// The partial applies of core's `remote.rs::apply`: `true` when the
/// record changed. All inputs were validated by [`check`]; a `false`
/// return is unreachable for documented kinds and the default for unknown
/// ones.
fn apply_partial(record: &mut Terminal, kind: &str, data: &Value) -> bool {
    match kind {
        event_kind::LOST | event_kind::FINISHED => {
            let end: EndData = serde_json::from_value(data.clone()).expect("checked");
            record.state = if kind == event_kind::LOST {
                "live".into()
            } else {
                "finished".into()
            };
            // Documented: lost clears the pid; finished keeps it.
            if kind == event_kind::LOST {
                record.pid = None;
            }
            record.reason = end.reason;
            record.exit_code = end.exit_code;
            record.signal = end.signal;
            record.finished_ms = end.finished_ms;
            record.foreground = None;
            true
        }
        event_kind::STATUS => {
            let status: StatusData = serde_json::from_value(data.clone()).expect("checked");
            record.status = status.status;
            record.records = status.records;
            record.reported = status.reported;
            true
        }
        event_kind::TURN => {
            record.turn = data.get("turn").and_then(Value::as_u64).expect("checked");
            true
        }
        event_kind::TITLE => {
            record.title = data
                .get("title")
                .and_then(Value::as_str)
                .expect("checked")
                .into();
            true
        }
        event_kind::KEEP => {
            record.keep = data.get("keep").and_then(Value::as_bool).expect("checked");
            true
        }
        event_kind::SIZE => {
            let cell = |key| {
                data[key]
                    .as_u64()
                    .and_then(|n| u16::try_from(n).ok())
                    .expect("checked")
            };
            record.cols = cell("cols");
            record.rows = cell("rows");
            true
        }
        event_kind::FOREGROUND => {
            record.foreground = serde_json::from_value(data.clone()).expect("checked");
            true
        }
        _ => false,
    }
}

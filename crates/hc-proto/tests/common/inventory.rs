//! Typed fixtures for the pure inventory reducer tests.

use hc_proto::events::EventParams;
use hc_proto::terminal::Terminal;
use serde_json::{Value, json};

/// Parses a `Terminal` from a record-shaped JSON value.
pub fn terminal(value: Value) -> Terminal {
    serde_json::from_value(value).unwrap()
}

/// A `Terminal` record carrying only the fields the schema requires.
pub fn term(address: &str) -> Terminal {
    let mut record = super::minimal_terminal();
    record["address"] = json!(address);
    terminal(record)
}

/// Typed `event` params — what `ServerNotification::Event` carries.
pub fn ev(seq: u64, kind: &str, terminal: Option<&str>, data: Value) -> EventParams {
    EventParams {
        seq,
        kind: kind.into(),
        terminal: terminal.map(str::to_string),
        machine: None,
        data,
    }
}

/// `lost`/`finished` data with all four documented members.
pub fn end_data() -> Value {
    json!({"reason": "exited", "exit_code": 0, "signal": null, "finished_ms": 1})
}

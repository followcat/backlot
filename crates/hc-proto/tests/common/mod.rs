//! Shared test fixtures. `mod common;` in each test target pulls this in.

use serde_json::json;

/// The smallest terminal record the schema allows; tests add fields on top.
pub fn minimal_terminal() -> serde_json::Value {
    json!({
        "address": "t7",
        "labels": {},
        "argv": ["sh"],
        "cwd": "/home/user",
        "state": "live",
        "created_ms": 1700000000000u64,
        "screen": "exact",
        "cols": 80,
        "rows": 24,
    })
}

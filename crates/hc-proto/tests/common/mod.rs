//! Shared test fixtures. `mod common;` in each test target pulls this in;
//! not every target uses every helper.
#![allow(dead_code)]

pub mod attach;
pub mod fakecore;
pub mod faults;
pub mod inventory;
pub mod subscribe;

use serde_json::json;

/// Discovery is a background task: waits until it settles — a document or
/// a note — so tests never race it.
pub async fn discovery_settled(client: &hc_proto::client::Client) {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        while client.discovery().is_none() && client.discovery_note().is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("discovery never settled");
}

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

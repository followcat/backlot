//! Shared subscription fixtures: params, open/join helpers, the fake
//! core's fixed opening, and notification line builders.

use hc_proto::events::{ServerNotification, SubscribeParams, SubscribeResult};
use hc_proto::subscribe::{SubscribeError, Subscription};
use hc_proto::transport::{ReadHalf, WriteHalf};
use serde_json::{Value, json};

#[allow(unused_imports)] // every test target compiles `common` on its own
pub use super::attach::{DEADLINE, joined};
use super::fakecore::DuplexCore;

/// An unfiltered `events.subscribe` request.
pub fn params() -> SubscribeParams {
    SubscribeParams::default()
}

/// A method list without `events.subscribe`, for the capability gate.
pub fn without_subscribe() -> &'static [&'static str] {
    &["hello", "terminal.list"]
}

/// A snapshot: `seq` plus the given terminal records.
pub fn snapshot(seq: u64, terminals: Vec<Value>) -> SubscribeResult {
    SubscribeResult {
        seq,
        terminals: terminals
            .into_iter()
            .map(|t| serde_json::from_value(t).unwrap())
            .collect(),
        channel: None,
    }
}

/// The same snapshot as a JSON reply `result`.
pub fn snapshot_value(seq: u64, terminals: Vec<Value>) -> Value {
    json!({"seq": seq, "terminals": terminals})
}

/// An `event` notification line's params.
pub fn event(seq: u64, kind: &str, terminal: Option<&str>, data: Value) -> Value {
    json!({"seq": seq, "type": kind, "terminal": terminal, "data": data})
}

/// A whole `event` notification line.
pub fn event_line(seq: u64, kind: &str, terminal: Option<&str>, data: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": "event", "params": event(seq, kind, terminal, data)})
}

/// A whole `heartbeat` notification line.
pub fn heartbeat_line(seq: u64) -> Value {
    json!({"jsonrpc": "2.0", "method": "heartbeat", "params": {"seq": seq}})
}

/// Joins the client side and returns it; the server half finishes inside.
pub async fn opening(read: ReadHalf, write: WriteHalf, p: &SubscribeParams) -> Subscription {
    tokio::time::timeout(DEADLINE, Subscription::open(read, write, None, p))
        .await
        .expect("subscribe open timed out")
        .expect("subscribe failed")
}

/// The fake's fixed opening: plain hello, the subscribe request, its
/// reply. Returns the request so a test can assert the encoded params.
pub async fn serve_subscribe(core: &mut DuplexCore, methods: &[&str], result: Value) -> Value {
    core.serve_plain_hello(methods).await;
    let request = core.read_message().await;
    assert_eq!(request["method"], "events.subscribe");
    core.reply_line(&request["id"], result).await;
    request
}

/// The next notification, deadline-bounded.
pub async fn recv(sub: &mut Subscription) -> ServerNotification {
    tokio::time::timeout(DEADLINE, sub.recv())
        .await
        .expect("recv timed out")
        .expect("recv failed")
}

/// The next notification's error, deadline-bounded.
pub async fn recv_err(sub: &mut Subscription) -> SubscribeError {
    tokio::time::timeout(DEADLINE, sub.recv())
        .await
        .expect("recv timed out")
        .expect_err("recv had to fail")
}

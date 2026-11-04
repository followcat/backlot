//! `rpc.discover` tests: bounded optional enrichment that refines method
//! availability, and honest notes for every unusable outcome. Scripted
//! fake cores on in-memory wires; every wait is bounded.

mod common;

use common::{discovery_settled, fakecore};
use hc_proto::client::{CallError, Client};
use hc_proto::terminal::ListParams;
use serde_json::json;
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(15);

async fn timeout<F: std::future::Future>(f: F) -> F::Output {
    tokio::time::timeout(DEADLINE, f)
        .await
        .expect("deadline exceeded")
}

async fn client_open(
    read: hc_proto::transport::ReadHalf,
    write: hc_proto::transport::WriteHalf,
) -> Client {
    timeout(Client::open(read, write, None)).await.unwrap()
}

#[tokio::test]
async fn stalled_discovery_is_bounded_and_never_blocks_calls() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list", "rpc.discover"]).await;
        for _ in 0..2 {
            let req = core.read_request().await;
            match req["method"].as_str().unwrap() {
                // Hold the discovery answer past its 5s budget; a working
                // call must still complete normally meanwhile.
                "rpc.discover" => tokio::time::sleep(Duration::from_secs(6)).await,
                "terminal.list" => {
                    core.reply(&req["id"], json!({"terminals": [], "hidden_finished": 0}))
                        .await
                }
                other => panic!("unexpected method {other}"),
            }
        }
        core
    });
    let client = client_open(read, write).await;

    // A working method answers promptly while discovery is still stalled.
    let list = timeout(client.terminal_list(ListParams::default()))
        .await
        .unwrap();
    assert!(list.terminals.is_empty());
    // Discovery then settles as a noted failure, not a hang.
    discovery_settled(&client).await;
    assert!(client.discovery().is_none());
    let note = client.discovery_note().unwrap();
    assert!(note.contains("rpc.discover"), "{note}");
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn discovery_refines_method_availability() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list", "rpc.discover"]).await;
        // The document exists but omits terminal.list — the core's own
        // declaration wins over hello's list.
        let req = core.read_request().await;
        assert_eq!(req["method"], "rpc.discover");
        core.reply(&req["id"], fakecore::discover_doc(&["rpc.discover"]))
            .await;
        core
    });
    let client = client_open(read, write).await;
    discovery_settled(&client).await;
    assert!(client.discovery().is_some());

    let err = timeout(client.terminal_list(ListParams::default()))
        .await
        .unwrap_err();
    let CallError::Unsupported { method, reason } = err else {
        panic!("expected Unsupported, got {err:?}");
    };
    assert_eq!(method, "terminal.list");
    assert!(reason.contains("rpc.discover"), "{reason}");
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn unusable_discovery_is_a_note_not_a_gate() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list", "rpc.discover"]).await;
        let req = core.read_request().await;
        assert_eq!(req["method"], "rpc.discover");
        // methods[0] lacks a string name: the document is unusable.
        core.reply(
            &req["id"],
            json!({"openrpc": "1.2.7", "methods": [{"name": 42}]}),
        )
        .await;
        let next = core.read_request().await;
        core.reply(&next["id"], json!({"terminals": [], "hidden_finished": 1}))
            .await;
        core
    });
    let client = client_open(read, write).await;
    discovery_settled(&client).await;
    assert!(client.discovery().is_none());
    let note = client.discovery_note().unwrap();
    assert!(note.contains("rpc.discover"), "{note}");

    // Hello capabilities alone still gate: the offered method works.
    let list = timeout(client.terminal_list(ListParams::default()))
        .await
        .unwrap();
    assert_eq!(list.hidden_finished, 1);
    timeout(server).await.unwrap();
}

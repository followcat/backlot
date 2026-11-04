//! Client concurrency tests: out-of-order replies, a held call, timeouts
//! and cancellation releasing capacity, queued writes staying whole, and
//! the enqueue/in-flight bounds. Scripted fake cores on in-memory wires;
//! every wait is bounded.

mod common;

use common::fakecore;
use hc_proto::client::{CallError, Client};
use hc_proto::terminal::{ListParams, ReadParams};
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
async fn out_of_order_replies_still_correlate() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list", "doc.get"]).await;
        // Collect both requests before answering the second first.
        let list = core.read_request().await;
        let doc = core.read_request().await;
        core.reply(&doc["id"], json!({"value": "doc", "version": 1}))
            .await;
        core.reply(&list["id"], json!({"terminals": [], "hidden_finished": 2}))
            .await;
        core
    });
    let client = client_open(read, write).await;

    let list_call = tokio::spawn({
        let client = client.clone();
        async move { client.terminal_list(ListParams::default()).await }
    });
    let doc_call = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .doc_get(hc_proto::doc::DocGetParams {
                    name: "d".into(),
                    on: None,
                })
                .await
        }
    });
    let (list, doc) = timeout(async { (list_call.await.unwrap(), doc_call.await.unwrap()) }).await;
    assert_eq!(list.unwrap().hidden_finished, 2);
    assert_eq!(doc.unwrap().version, 1);
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn a_held_call_does_not_stall_others() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list", "terminal.read"]).await;
        let slow = core.read_request().await;
        let fast = core.read_request().await;
        // Answer the fast one first; hold the slow one a while.
        core.reply(&fast["id"], json!({"terminals": [], "hidden_finished": 0}))
            .await;
        tokio::time::sleep(Duration::from_millis(250)).await;
        core.reply(
            &slow["id"],
            json!({"text": "later", "complete": true, "how": "screen"}),
        )
        .await;
        core
    });
    let client = client_open(read, write).await;

    let slow = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .terminal_read(ReadParams {
                    terminal: "t7".into(),
                    lines: 0,
                    caller: None,
                })
                .await
        }
    });
    tokio::task::yield_now().await;
    let fast = tokio::spawn(async move { client.terminal_list(ListParams::default()).await });
    // The fast call finishes well inside the slow one's delay.
    let fast_out = tokio::time::timeout(Duration::from_secs(5), fast)
        .await
        .expect("fast call stalled behind the held one")
        .unwrap()
        .unwrap();
    assert!(fast_out.terminals.is_empty());
    let slow_out = timeout(slow).await.unwrap().unwrap();
    assert_eq!(slow_out.text, "later");
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn timeout_releases_capacity_and_late_replies_are_dropped() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list", "doc.get"]).await;
        // The doc.get request arrives; hold its answer past the call timeout.
        let held = core.read_request().await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        core.reply(&held["id"], json!({"value": null, "version": 0}))
            .await;
        // Then serve the next call normally.
        let next = core.read_request().await;
        core.reply(&next["id"], json!({"terminals": [], "hidden_finished": 0}))
            .await;
        core
    });
    let client = client_open(read, write).await;

    let err = timeout(client.call_timeout(
        "doc.get",
        Some(json!({"name": "d"})),
        Duration::from_millis(50),
    ))
    .await
    .unwrap_err();
    assert!(matches!(err, CallError::Timeout));
    // The pending entry is gone and capacity was released.
    assert_eq!(client.in_flight(), 0);
    // The late reply is dropped safely; the next call still works.
    let list = timeout(client.terminal_list(ListParams::default()))
        .await
        .unwrap();
    assert_eq!(client.in_flight(), 0);
    assert!(list.terminals.is_empty());
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn a_cancelled_caller_releases_its_pending_entry() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list", "doc.get"]).await;
        let _abandoned = core.read_request().await; // never answered
        let next = core.read_request().await;
        core.reply(&next["id"], json!({"terminals": [], "hidden_finished": 0}))
            .await;
        core
    });
    let client = client_open(read, write).await;

    let abandoned = tokio::spawn({
        let client = client.clone();
        async move { client.call("doc.get", Some(json!({"name": "d"}))).await }
    });
    tokio::task::yield_now().await;
    abandoned.abort(); // the caller cancels: receiver dropped
    let list = timeout(client.terminal_list(ListParams::default()))
        .await
        .unwrap();
    assert!(list.terminals.is_empty());
    // The dispatch swept the dead entry away.
    assert_eq!(client.in_flight(), 0);
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn cancelled_callers_release_their_pending_entries() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list"]).await;
        core.read_request().await; // never answered; halves stay alive
        tokio::time::sleep(Duration::from_secs(60)).await;
    });
    let client = client_open(read, write).await;

    // After enqueue: the caller drops mid-wait; its slot frees promptly,
    // with no peer activity and no later call to sweep it.
    let call = tokio::spawn({
        let client = client.clone();
        async move { client.terminal_list(ListParams::default()).await }
    });
    timeout(async {
        while client.in_flight() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    call.abort();
    timeout(async {
        while client.in_flight() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    server.abort();
}

#[tokio::test]
async fn queued_writes_stay_whole_when_a_caller_cancels() {
    // A tiny pipe: the writer blocks mid-frame until the peer drains.
    let (mut core, read, write) = fakecore::pair(16);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list", "doc.get"]).await;
        // Don't read for a while: the writer task stalls mid-frame.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let first = core.read_request().await;
        let second = core.read_request().await;
        // Whatever arrived, each frame must be one complete JSON request.
        (
            first["method"].as_str().unwrap().to_string(),
            second["method"].as_str().unwrap().to_string(),
        )
    });
    let client = client_open(read, write).await;

    let call_a = tokio::spawn({
        let client = client.clone();
        async move { client.call("terminal.list", None).await }
    });
    tokio::task::yield_now().await; // A is enqueued before B, deterministically
    let call_b = tokio::spawn({
        let client = client.clone();
        async move { client.call("doc.get", Some(json!({"name": "d"}))).await }
    });
    // Cancel the second caller while its frame is queued/blocked.
    tokio::time::sleep(Duration::from_millis(50)).await;
    call_b.abort();
    let (first, second) = timeout(server).await.unwrap();
    assert_eq!(first, "terminal.list");
    assert_eq!(second, "doc.get", "the queued frame was written whole");
    call_a.abort(); // no reply is coming; keep the test fast
}

#[tokio::test]
async fn enqueue_waits_inside_the_call_deadline() {
    // A peer that stops reading after hello: the pipe fills, the bounded
    // queue fills, and further sends block.
    let (mut core, read, write) = fakecore::pair(64);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list"]).await;
        tokio::time::sleep(Duration::from_secs(60)).await;
        core
    });
    let client = client_open(read, write).await;

    let mut calls = Vec::new();
    for _ in 0..66 {
        calls.push(tokio::spawn({
            let client = client.clone();
            async move { client.terminal_list(ListParams::default()).await }
        }));
    }
    timeout(async {
        while client.in_flight() < 66 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    // The queue and pipe are now full: this call's *enqueue* blocks, and
    // its 150ms budget still applies — not the 120s default.
    let err = timeout(client.call_timeout("terminal.list", None, Duration::from_millis(150)))
        .await
        .unwrap_err();
    assert!(matches!(err, CallError::Timeout), "{err:?}");
    // Its pending entry is gone; the 66 blocked callers keep their slots.
    assert_eq!(client.in_flight(), 66);
    for call in calls {
        call.abort();
    }
    server.abort();
    timeout(client.close()).await.unwrap();
}

#[tokio::test]
async fn a_cancelled_blocked_sender_releases_its_slot() {
    // Cancellation before enqueue: the peer is not reading, the queue and
    // pipe are full, so this caller is parked in tx.send when aborted.
    let (mut core, read, write) = fakecore::pair(64);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list"]).await;
        tokio::time::sleep(Duration::from_secs(60)).await;
        core
    });
    let client = client_open(read, write).await;

    let mut blockers = Vec::new();
    for _ in 0..66 {
        blockers.push(tokio::spawn({
            let client = client.clone();
            async move { client.terminal_list(ListParams::default()).await }
        }));
    }
    timeout(async {
        while client.in_flight() < 66 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    let parked = tokio::spawn({
        let client = client.clone();
        async move { client.terminal_list(ListParams::default()).await }
    });
    timeout(async {
        while client.in_flight() < 67 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    parked.abort();
    timeout(async {
        while client.in_flight() != 66 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    for call in blockers {
        call.abort();
    }
    server.abort();
    timeout(client.close()).await.unwrap();
}

#[tokio::test]
async fn in_flight_bound_rejects_new_calls() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list"]).await;
        // Never answer: every call stays in flight.
        tokio::time::sleep(Duration::from_secs(60)).await;
        core
    });
    let client = client_open(read, write).await;

    let mut calls = Vec::new();
    for _ in 0..256 {
        calls.push(tokio::spawn({
            let client = client.clone();
            async move { client.terminal_list(ListParams::default()).await }
        }));
    }
    // The spawned callers register on their first poll: wait until every
    // one is in flight (single-threaded runtime — yield to let them run).
    timeout(async {
        while client.in_flight() < 256 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    // The 257th call is refused without touching the wire.
    let err = timeout(client.terminal_list(ListParams::default()))
        .await
        .unwrap_err();
    assert!(matches!(err, CallError::Full), "{err:?}");
    for call in calls {
        call.abort();
    }
    server.abort();
    timeout(client.close()).await.unwrap();
}

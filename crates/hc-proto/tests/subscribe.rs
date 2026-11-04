//! The dedicated plain-mode `events.subscribe` handshake: negotiated
//! without a framed switch, one typed request, and a snapshot reply that
//! must be the wire's next message. Scripted fake cores over duplex
//! pairs; every wait is deadline-bounded.

mod common;

use common::fakecore::{ALL_METHODS, pair};
use common::minimal_terminal;
use common::subscribe::{
    self, DEADLINE, event_line, heartbeat_line, joined, opening, params, serve_subscribe,
    without_subscribe,
};
use hc_proto::client::CallError;
use hc_proto::events::{ServerNotification, SubscribeParams};
use hc_proto::subscribe::{SubscribeError, Subscription};
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn plain_hello_and_subscribe_request() {
    let (core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let mut core = core;
        core.serve_plain_hello(ALL_METHODS).await;
        let request = core.read_message().await;
        assert_eq!(request["id"], 1);
        assert_eq!(request["method"], "events.subscribe");
        core.reply_line(
            &request["id"],
            subscribe::snapshot_value(7, vec![minimal_terminal()]),
        )
        .await;
        core.expect_eof_line().await;
        core
    });
    let p = params();
    let sub = opening(read, write, &p).await;
    assert!(!sub.hello().framed);
    assert!(!sub.filtered());
    assert_eq!(sub.snapshot().seq, 7);
    assert_eq!(sub.snapshot().terminals[0].address, "t7");
    drop(sub);
    joined(server).await;
}

#[tokio::test]
async fn subscribe_params_encode_filters() {
    let (core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let mut core = core;
        let request =
            serve_subscribe(&mut core, ALL_METHODS, subscribe::snapshot_value(1, vec![])).await;
        assert_eq!(request["params"]["terminal"], "t7");
        assert_eq!(request["params"]["children_of"], "t3");
        assert_eq!(request["params"]["labels"]["herdr.space"], "work");
        core.expect_eof_line().await;
        core
    });
    let p = SubscribeParams {
        terminal: Some("t7".into()),
        children_of: Some("t3".into()),
        labels: [("herdr.space".to_string(), "work".to_string())].into(),
    };
    let sub = opening(read, write, &p).await;
    assert!(sub.filtered());
    drop(sub);
    joined(server).await;
}

#[tokio::test]
async fn missing_method_is_unsupported() {
    let (core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let mut core = core;
        core.serve_plain_hello(without_subscribe()).await;
        core.expect_eof_line().await;
        core
    });
    let p = params();
    let err = tokio::time::timeout(DEADLINE, Subscription::open(read, write, None, &p))
        .await
        .expect("open timed out")
        .expect_err("subscribe had to fail");
    assert!(
        matches!(err, SubscribeError::Call(CallError::Unsupported { ref method, .. }) if method == "events.subscribe"),
        "{err:?}"
    );
    joined(server).await;
}

#[tokio::test]
async fn subscribe_rpc_error_keeps_its_kind() {
    let (core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let mut core = core;
        core.serve_plain_hello(ALL_METHODS).await;
        let request = core.read_message().await;
        core.reply_line_error(&request["id"], "denied", "no events for you")
            .await;
        core.expect_eof_line().await;
        core
    });
    let p = params();
    let err = tokio::time::timeout(DEADLINE, Subscription::open(read, write, None, &p))
        .await
        .expect("open timed out")
        .expect_err("subscribe had to fail");
    let SubscribeError::Call(CallError::Rpc(rpc)) = err else {
        panic!("expected a structured rpc error: {err:?}");
    };
    assert!(rpc.is_kind("denied"), "{rpc}");
    joined(server).await;
}

#[tokio::test]
async fn a_plain_reply_with_a_channel_is_rejected() {
    let (core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let mut core = core;
        serve_subscribe(
            &mut core,
            ALL_METHODS,
            json!({"seq": 1, "terminals": [], "channel": 5}),
        )
        .await;
        core.expect_eof_line().await;
        core
    });
    let p = params();
    let err = tokio::time::timeout(DEADLINE, Subscription::open(read, write, None, &p))
        .await
        .expect("open timed out")
        .expect_err("a channel on a plain wire had to fail");
    assert!(
        matches!(err, SubscribeError::Call(CallError::Protocol(_))),
        "{err:?}"
    );
    joined(server).await;
}

#[tokio::test]
async fn a_notification_before_the_reply_is_a_violation() {
    // The contract sends the snapshot reply first: a notification ahead of
    // it is never queued — otherwise a peer could flood memory inside the
    // subscribe deadline. Post-reply read-ahead stays valid.
    let (core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let mut core = core;
        core.serve_plain_hello(ALL_METHODS).await;
        let request = core.read_message().await;
        assert_eq!(request["method"], "events.subscribe");
        // A whole flood of pre-reply notifications, then the reply: the
        // client must fail on the first one, not buffer them all.
        for seq in 0..1000u64 {
            if !core.try_send_line(&heartbeat_line(seq)).await {
                break; // the client already hung up on the violation
            }
        }
        let _ = core
            .try_send_line(&json!({
                "jsonrpc": "2.0", "id": request["id"],
                "result": subscribe::snapshot_value(1, vec![]),
            }))
            .await;
        core.expect_eof_line().await;
        core
    });
    let p = params();
    let err = tokio::time::timeout(DEADLINE, Subscription::open(read, write, None, &p))
        .await
        .expect("open timed out")
        .expect_err("a pre-reply notification had to fail");
    assert!(
        matches!(err, SubscribeError::Call(CallError::Protocol(_))),
        "{err:?}"
    );
    joined(server).await;
}

#[tokio::test]
async fn coalesced_reply_and_first_notifications() {
    // The reply and two notifications arrive in ONE read: nothing after
    // the reply's newline may be dropped.
    let (core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let mut core = core;
        core.serve_plain_hello(ALL_METHODS).await;
        let request = core.read_message().await;
        let mut packet = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": request["id"],
            "result": subscribe::snapshot_value(3, vec![]),
        }))
        .unwrap();
        packet.push(b'\n');
        for message in [
            event_line(4, "title", Some("t7"), json!({"title": "one"})),
            heartbeat_line(4),
        ] {
            packet.extend(serde_json::to_vec(&message).unwrap());
            packet.push(b'\n');
        }
        core.send(&packet).await;
        core.expect_eof_line().await;
        core
    });
    let p = params();
    let mut sub = opening(read, write, &p).await;
    match subscribe::recv(&mut sub).await {
        ServerNotification::Event(e) => {
            assert_eq!(e.seq, 4);
            assert_eq!(e.kind, "title");
        }
        other => panic!("expected the coalesced event, got {other:?}"),
    }
    assert!(matches!(
        subscribe::recv(&mut sub).await,
        ServerNotification::Heartbeat(_)
    ));
    drop(sub);
    joined(server).await;
}

#[tokio::test]
async fn cancelling_open_during_subscribe_ends_the_wire() {
    // No background tasks exist: dropping the open future drops the halves
    // synchronously — the peer sees EOF.
    let (core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let mut core = core;
        core.serve_plain_hello(ALL_METHODS).await;
        let request = core.read_message().await;
        assert_eq!(request["method"], "events.subscribe");
        // Never answered; the open is cancelled while parked on the reply.
        core.expect_eof_line().await;
        core
    });
    let p = params();
    let opening = tokio::spawn(async move {
        let _ = Subscription::open(read, write, None, &p).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    opening.abort();
    assert!(
        opening
            .await
            .expect_err("the open task had to cancel")
            .is_cancelled()
    );
    joined(server).await;
}

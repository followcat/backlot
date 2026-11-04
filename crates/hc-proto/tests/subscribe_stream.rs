//! The subscribed wire after the snapshot reply: notifications only —
//! unknown methods ignored, unknown event kinds retained, fragmented and
//! coalesced bytes preserved, and anything malformed a *latched* terminal
//! error that ends the wire instead of resuming on an untrusted stream.
//! Scripted fake cores over duplex pairs; every wait is deadline-bounded.

mod common;

use common::fakecore::{ALL_METHODS, pair};
use common::minimal_terminal;
use common::subscribe::{
    self, DEADLINE, event_line, heartbeat_line, joined, opening, params, recv, recv_err,
    serve_subscribe,
};
use hc_proto::events::ServerNotification;
use hc_proto::subscribe::SubscribeError;
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn fragmented_notification_lines() {
    let (core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let mut core = core;
        serve_subscribe(&mut core, ALL_METHODS, subscribe::snapshot_value(1, vec![])).await;
        let line = serde_json::to_vec(&heartbeat_line(9)).unwrap();
        let half = line.len() / 2;
        core.send(&line[..half]).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        let mut tail = line[half..].to_vec();
        tail.push(b'\n');
        core.send(&tail).await;
        core.expect_eof_line().await;
        core
    });
    let p = params();
    let mut sub = opening(read, write, &p).await;
    match recv(&mut sub).await {
        ServerNotification::Heartbeat(h) => assert_eq!(h.seq, 9),
        other => panic!("expected a heartbeat, got {other:?}"),
    }
    drop(sub);
    joined(server).await;
}

#[tokio::test]
async fn unknown_methods_and_kinds_are_tolerated() {
    let (core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let mut core = core;
        serve_subscribe(&mut core, ALL_METHODS, subscribe::snapshot_value(1, vec![])).await;
        core.notify("future.notice", json!({"x": 1})).await;
        core.send_line(&event_line(2, "newkind", Some("t7"), json!({"z": 1})))
            .await;
        core.expect_eof_line().await;
        core
    });
    let p = params();
    let mut sub = opening(read, write, &p).await;
    match recv(&mut sub).await {
        // The foreign method was skipped; the new event kind kept its name.
        ServerNotification::Event(e) => assert_eq!(e.kind, "newkind"),
        other => panic!("the foreign method had to be skipped: {other:?}"),
    }
    drop(sub);
    joined(server).await;
}

#[tokio::test]
async fn heartbeat_and_missed_decode() {
    let (core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let mut core = core;
        serve_subscribe(&mut core, ALL_METHODS, subscribe::snapshot_value(5, vec![])).await;
        core.send_line(&heartbeat_line(5)).await;
        core.notify(
            "missed",
            json!({"seq": 8, "terminals": [minimal_terminal()], "machine": "host-b"}),
        )
        .await;
        core.expect_eof_line().await;
        core
    });
    let p = params();
    let mut sub = opening(read, write, &p).await;
    match recv(&mut sub).await {
        ServerNotification::Heartbeat(h) => assert_eq!(h.seq, 5),
        other => panic!("expected a heartbeat, got {other:?}"),
    }
    match recv(&mut sub).await {
        ServerNotification::Missed(m) => {
            assert_eq!(m.seq, 8);
            assert_eq!(m.terminals.len(), 1);
            assert_eq!(m.machine.as_deref(), Some("host-b"));
        }
        other => panic!("expected missed, got {other:?}"),
    }
    drop(sub);
    joined(server).await;
}

#[tokio::test]
async fn a_malformed_line_is_terminal() {
    let (core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let mut core = core;
        serve_subscribe(&mut core, ALL_METHODS, subscribe::snapshot_value(1, vec![])).await;
        core.send(b"{not json\n").await;
        core.expect_eof_line().await;
        core
    });
    let p = params();
    let mut sub = opening(read, write, &p).await;
    assert!(matches!(
        recv_err(&mut sub).await,
        SubscribeError::Protocol(_)
    ));
    drop(sub);
    joined(server).await;
}

#[tokio::test]
async fn a_non_notification_envelope_is_terminal() {
    let (core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let mut core = core;
        serve_subscribe(&mut core, ALL_METHODS, subscribe::snapshot_value(1, vec![])).await;
        core.send_line(&json!({"jsonrpc": "2.0", "id": 9, "result": {}}))
            .await;
        core.expect_eof_line().await;
        core
    });
    let p = params();
    let mut sub = opening(read, write, &p).await;
    assert!(matches!(
        recv_err(&mut sub).await,
        SubscribeError::Protocol(_)
    ));
    drop(sub);
    joined(server).await;
}

#[tokio::test]
async fn malformed_required_params_are_terminal() {
    // A `missed` without `terminals` and an `event` without `seq` are both
    // malformed, never an empty inventory or a dropped message.
    for (method, bad) in [
        ("missed", json!({"seq": 8})),
        (
            "event",
            json!({"type": "title", "terminal": "t7", "data": {"title": "x"}}),
        ),
    ] {
        let (core, read, write) = pair(64 * 1024);
        let server = tokio::spawn(async move {
            let mut core = core;
            serve_subscribe(&mut core, ALL_METHODS, subscribe::snapshot_value(1, vec![])).await;
            core.notify(method, bad).await;
            core.expect_eof_line().await;
            core
        });
        let p = params();
        let mut sub = opening(read, write, &p).await;
        assert!(
            matches!(recv_err(&mut sub).await, SubscribeError::Protocol(_)),
            "{method} params had to be rejected"
        );
        drop(sub);
        joined(server).await;
    }
}

#[tokio::test]
async fn a_known_event_without_data_is_malformed_null_is_not() {
    // `data: null` is a real payload — `foreground` uses it for "no
    // foreground". A documented kind omitting `data` entirely is
    // malformed wire data and terminal; an unknown kind may still omit
    // it and stay tolerated.
    let (core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let mut core = core;
        serve_subscribe(&mut core, ALL_METHODS, subscribe::snapshot_value(1, vec![])).await;
        core.send_line(&json!({
            "jsonrpc": "2.0", "method": "event",
            "params": {"seq": 2, "type": "foreground", "terminal": "t7", "data": null},
        }))
        .await;
        core.send_line(&json!({
            "jsonrpc": "2.0", "method": "event",
            "params": {"seq": 3, "type": "madeup"},
        }))
        .await;
        core.send_line(&json!({
            "jsonrpc": "2.0", "method": "event",
            "params": {"seq": 4, "type": "foreground", "terminal": "t7"},
        }))
        .await;
        core.expect_eof_line().await;
        core
    });
    let p = params();
    let mut sub = opening(read, write, &p).await;
    let ServerNotification::Event(event) = recv(&mut sub).await else {
        panic!("an explicit null is the foreground-cleared event")
    };
    assert_eq!(event.kind, "foreground");
    assert!(event.data.is_null());
    let ServerNotification::Event(event) = recv(&mut sub).await else {
        panic!("an unknown kind may omit data")
    };
    assert_eq!(event.kind, "madeup");
    assert!(matches!(
        recv_err(&mut sub).await,
        SubscribeError::Protocol(_)
    ));
    joined(server).await;
}

#[tokio::test]
async fn a_terminal_error_latches_and_never_resumes() {
    // Malformed line, then a perfectly valid notification: the first recv
    // reports the violation and ends the wire — the later message must NOT
    // be delivered on a resumed, untrusted stream.
    let (core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let mut core = core;
        serve_subscribe(&mut core, ALL_METHODS, subscribe::snapshot_value(1, vec![])).await;
        core.send(b"{not json\n").await;
        core.send_line(&heartbeat_line(9)).await;
        core.expect_eof_line().await;
        core
    });
    let p = params();
    let mut sub = opening(read, write, &p).await;
    assert!(matches!(
        recv_err(&mut sub).await,
        SubscribeError::Protocol(_)
    ));
    // Latched: prompt `Closed`, never the queued valid heartbeat.
    for _ in 0..3 {
        assert!(matches!(recv_err(&mut sub).await, SubscribeError::Closed));
    }
    // `close` after a latched end has nothing left to report.
    let closed = tokio::time::timeout(DEADLINE, sub.close())
        .await
        .expect("close timed out")
        .expect("close failed");
    assert!(closed.status.is_none());
    joined(server).await;
}

#[tokio::test]
async fn eof_also_latches() {
    let (core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let mut core = core;
        serve_subscribe(&mut core, ALL_METHODS, subscribe::snapshot_value(1, vec![])).await;
        drop(core);
    });
    let p = params();
    let mut sub = opening(read, write, &p).await;
    assert!(matches!(recv_err(&mut sub).await, SubscribeError::Closed));
    assert!(matches!(recv_err(&mut sub).await, SubscribeError::Closed));
    joined(server).await;
}

#[tokio::test]
async fn a_healthy_recv_cancel_keeps_the_stream() {
    // A cancelled `recv` must not poison the reader: the buffered state
    // survives and the next `recv` serves the next notification.
    let (core, read, write) = pair(64 * 1024);
    let (partial_sent, partial_rx) = tokio::sync::oneshot::channel();
    let (release, release_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let mut core = core;
        serve_subscribe(&mut core, ALL_METHODS, subscribe::snapshot_value(1, vec![])).await;
        let line = serde_json::to_vec(&heartbeat_line(4)).unwrap();
        let half = line.len() / 2;
        core.send(&line[..half]).await;
        let _ = partial_sent.send(());
        let _ = release_rx.await;
        let mut tail = line[half..].to_vec();
        tail.push(b'\n');
        core.send(&tail).await;
        core.expect_eof_line().await;
        core
    });
    let p = params();
    let mut sub = opening(read, write, &p).await;
    tokio::time::timeout(DEADLINE, partial_rx)
        .await
        .expect("the partial notification was never sent")
        .unwrap();
    // The server holds the tail until cancellation; scheduling cannot
    // race a complete notification against this deadline.
    assert!(
        tokio::time::timeout(Duration::from_millis(30), sub.recv())
            .await
            .is_err()
    );
    let _ = release.send(());
    match recv(&mut sub).await {
        ServerNotification::Heartbeat(h) => assert_eq!(h.seq, 4),
        other => panic!("a cancelled healthy read must keep the stream: {other:?}"),
    }
    drop(sub);
    joined(server).await;
}

#[tokio::test]
async fn close_ends_the_wire() {
    let (core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let mut core = core;
        serve_subscribe(&mut core, ALL_METHODS, subscribe::snapshot_value(1, vec![])).await;
        core.expect_eof_line().await;
        core
    });
    let p = params();
    let sub = opening(read, write, &p).await;
    let closed = tokio::time::timeout(DEADLINE, sub.close())
        .await
        .expect("close timed out")
        .expect("close failed");
    assert!(closed.status.is_none());
    joined(server).await;
}

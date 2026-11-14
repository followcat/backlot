//! A `terminal.attach` reply that fails `AttachResult` shape already
//! allocated its viewer channel on the core: the shared connection must
//! still CLOSE that channel — a plain error return would leak the viewer
//! for the connection's whole life.

mod common;

use common::attach::{joined, params, shared_once};
use common::fakecore::{ALL_METHODS, DuplexCore, attach_result};
use common::minimal_terminal;
use hc_proto::client::{AttachError, CallError, Output};
use hc_proto::frame::Kind;
use serde_json::{Value, json};

/// Reads the next attach request — a CLOSE arriving first fails the
/// test: no close may precede the anchor's own drop.
async fn next_request(core: &mut DuplexCore) -> Value {
    loop {
        let frame = core.read_frame().await;
        match (frame.channel, frame.kind) {
            (0, Kind::Data) => return serde_json::from_slice(&frame.payload).unwrap(),
            (channel, Kind::Close) => panic!("unexpected CLOSE for channel {channel}"),
            _ => {}
        }
    }
}

/// The reply named channel 7 but its terminal record is junk: the open
/// fails `Shape` *and* closes channel 7 — the core already allocated the
/// viewer, so a silent error would leak it until the connection died.
#[tokio::test]
async fn a_malformed_attach_reply_closes_the_allocated_channel() {
    let (conn, mut core, _) = shared_once();
    let server = tokio::spawn(async move {
        core.serve_hello(ALL_METHODS).await;
        // The anchor attach on channel 5 keeps the connection alive, so
        // the test sees the leaked-channel close — not the teardown.
        let req = next_request(&mut core).await;
        core.reply(
            &req["id"],
            attach_result(5, minimal_terminal(), false, false),
        )
        .await;
        let req = next_request(&mut core).await;
        core.reply(
            &req["id"],
            json!({"channel": 7, "terminal": "not-an-object"}),
        )
        .await;
        // The reply's channel field is read leniently: even a failed
        // `AttachResult` shape still owes the allocated channel a CLOSE.
        let close = core.read_frame().await;
        assert_eq!(
            (close.channel, close.kind),
            (7, Kind::Close),
            "a shaped attach reply must still close its channel"
        );
        // The failed open did not touch the rest of the connection.
        core.send_stream(5, 0, b"anchor-alive").await;
        // The anchor's drop ends the connection; its own CLOSE races the
        // teardown (best-effort, like every last-claim drop) — only a
        // stray close on another channel would be a bug.
        while let Some(frame) = core.try_read_frame().await {
            if frame.kind == Kind::Close {
                assert_eq!(frame.channel, 5);
            }
        }
    });
    let anchor = conn.attach(&params()).await.expect("anchor attach");
    let err = conn
        .attach(&params())
        .await
        .expect_err("the malformed reply must fail");
    assert!(
        matches!(err, AttachError::Call(CallError::Shape(_))),
        "expected a shape failure, got {err:?}"
    );
    assert_eq!(
        anchor.recv().await,
        Some(Output::Data(b"anchor-alive".to_vec()))
    );
    drop(anchor);
    joined(server).await;
}

/// Shape failures that name channel 0 — or no channel at all — must not
/// fabricate a CLOSE 0 onto the control channel: only the anchor's own
/// channel ever closes.
#[tokio::test]
async fn a_shape_failure_without_a_channel_sends_no_close() {
    let (conn, mut core, _) = shared_once();
    let server = tokio::spawn(async move {
        core.serve_hello(ALL_METHODS).await;
        let req = next_request(&mut core).await;
        core.reply(
            &req["id"],
            attach_result(5, minimal_terminal(), false, false),
        )
        .await;
        // Channel 0 is the control channel — never a closeable viewer.
        let req = next_request(&mut core).await;
        core.reply(
            &req["id"],
            json!({"channel": 0, "terminal": "not-an-object"}),
        )
        .await;
        // No channel field at all: nothing to close either.
        let req = next_request(&mut core).await;
        core.reply(&req["id"], json!({"terminal": "not-an-object"}))
            .await;
        // Everything until EOF: the only close allowed is the anchor's
        // own — a fabricated CLOSE 0 (or one on a missing channel) fails
        // here, as it would inside `next_request` while opens were live.
        while let Some(frame) = core.try_read_frame().await {
            if frame.kind == Kind::Close {
                assert_eq!(frame.channel, 5, "a bogus CLOSE must never leave");
            }
        }
    });
    let anchor = conn.attach(&params()).await.expect("anchor attach");
    for _ in 0..2 {
        let err = conn
            .attach(&params())
            .await
            .expect_err("the malformed reply must fail");
        assert!(
            matches!(err, AttachError::Call(CallError::Shape(_))),
            "expected a shape failure, got {err:?}"
        );
    }
    drop(anchor);
    joined(server).await;
}

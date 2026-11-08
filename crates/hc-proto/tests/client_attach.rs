//! Attach wire behavior: request encoding, first-paint preservation,
//! channel routing and ordering, held-frame replay and open failures —
//! all against scripted fake cores.

mod common;

use common::attach::{DEADLINE, joined, observer, opening, params};
use common::fakecore::{ALL_METHODS, DuplexCore, attach_result, pair};
use common::minimal_terminal;
use hc_proto::client::{Attach, AttachError, Output};
use hc_proto::frame::Frame;
use hc_proto::terminal::Display;
use serde_json::{Value, json};
use tokio::task::JoinHandle;

/// Joins a server returning its captured attach request.
async fn joined_with_req(server: JoinHandle<(DuplexCore, Value)>) -> (DuplexCore, Value) {
    tokio::time::timeout(DEADLINE, server)
        .await
        .expect("fake core task timed out")
        .expect("fake core task panicked")
}

#[tokio::test]
async fn attach_params_encode_for_the_core() {
    let (mut core, read, write) = pair(64 * 1024);
    let mut p = params();
    p.history = 200;
    p.caller = Some("t9".into());
    p.display = Some(Display {
        bg: Some("#112233".into()),
        cell: Some([9, 18]),
        cursor: None,
        fg: Some("#ddeeff".into()),
    });
    let server = tokio::spawn(async move {
        let req = core
            .serve_attach(
                ALL_METHODS,
                &["automatic_input"],
                attach_result(3, minimal_terminal(), false, true),
            )
            .await;
        core.drain_until_eof().await;
        (core, req)
    });
    let attach = opening(read, write, &p).await;
    assert_eq!(attach.channel(), 3);
    assert_eq!(attach.terminal().address, "t7");
    assert_eq!(attach.terminal().cols, 80);
    drop(attach);
    let (_core, req) = joined_with_req(server).await;
    let pp = &req["params"];
    assert_eq!(pp["terminal"], "t7");
    assert_eq!(pp["history"], 200);
    assert_eq!(pp["cols"], 80);
    assert_eq!(pp["rows"], 24);
    assert_eq!(pp["caller"], "t9");
    assert_eq!(pp["display"]["bg"], "#112233");
    assert_eq!(pp["display"]["cell"], json!([9, 18]));
    assert_eq!(pp["display"]["fg"], "#ddeeff");
    assert!(pp.get("observe").is_none(), "false observe is omitted");
    assert!(pp.get("resumed").is_none(), "false resumed is omitted");
}

#[tokio::test]
async fn observer_params_and_confirmed_flags() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let req = core
            .serve_attach(
                ALL_METHODS,
                &["automatic_input", "resize_notifications"],
                attach_result(1, minimal_terminal(), true, true),
            )
            .await;
        core.drain_until_eof().await;
        (core, req)
    });
    let attach = opening(read, write, &observer()).await;
    assert!(attach.observe());
    assert!(attach.resize_notifications());
    assert!(attach.automatic_input());
    drop(attach);
    let (_core, req) = joined_with_req(server).await;
    assert_eq!(req["params"]["observe"], true);
    assert_eq!(req["params"]["resize_notifications"], true);
}

#[tokio::test]
async fn reply_and_first_paint_in_one_read_survive() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(ALL_METHODS).await;
        let req = core.read_request().await;
        // The reply frame and the first DATA frame land in ONE write —
        // the read-ahead codec must hand both back, paint first.
        let reply = Frame::data(
            0,
            serde_json::to_vec(&json!({
                "jsonrpc": "2.0", "id": req["id"],
                "result": attach_result(7, minimal_terminal(), false, false),
            }))
            .unwrap(),
        );
        let paint = Frame::data(7, b"\x1b[2Jfirst-paint".to_vec());
        let mut one_write = reply.encode();
        one_write.extend_from_slice(&paint.encode());
        core.send(&one_write).await;
        core.drain_until_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    assert_eq!(attach.channel(), 7);
    let first = attach.recv().await.expect("first paint missing");
    assert_eq!(first, Output::Data(b"\x1b[2Jfirst-paint".to_vec()));
    drop(attach);
    joined(server).await;
}

#[tokio::test]
async fn a_fragmented_first_paint_arrives_whole() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(1, minimal_terminal(), false, false),
        )
        .await;
        // The paint's frame arrives split across three writes — header
        // and payload boundaries included — and must surface whole.
        let paint = Frame::data(1, b"fragmented-paint".to_vec()).encode();
        core.send(&paint[..5]).await;
        core.send(&paint[5..9]).await;
        core.send(&paint[9..]).await;
        core.drain_until_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    assert_eq!(
        attach.recv().await,
        Some(Output::Data(b"fragmented-paint".to_vec()))
    );
    drop(attach);
    joined(server).await;
}

#[tokio::test]
async fn resize_stays_before_the_paint_it_replaces() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &["resize_notifications"],
            attach_result(1, minimal_terminal(), true, false),
        )
        .await;
        core.send_stream(1, 3, &[100, 0, 30, 0]).await;
        core.send_stream(1, 0, b"paint-a").await;
        core.send_stream(1, 3, &[80, 0, 24, 0]).await;
        core.send_stream(1, 0, b"paint-b").await;
        core.drain_until_eof().await;
        core
    });
    let attach = opening(read, write, &observer()).await;
    assert_eq!(
        attach.recv().await,
        Some(Output::Resize {
            cols: 100,
            rows: 30
        })
    );
    assert_eq!(attach.recv().await, Some(Output::Data(b"paint-a".to_vec())));
    assert_eq!(
        attach.recv().await,
        Some(Output::Resize { cols: 80, rows: 24 })
    );
    assert_eq!(attach.recv().await, Some(Output::Data(b"paint-b".to_vec())));
    drop(attach);
    joined(server).await;
}

#[tokio::test]
async fn unknown_kinds_surface_and_foreign_channels_stay_routed() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(1, minimal_terminal(), false, false),
        )
        .await;
        core.send_stream(1, 42, b"mystery").await; // unknown kind, our channel
        core.send_stream(9, 0, b"not ours").await; // foreign channel: held for channel 9
        core.send_stream(1, 0, b"ours").await;
        core.drain_until_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    assert_eq!(
        attach.recv().await,
        Some(Output::Control {
            channel: 1,
            kind: 42,
            payload: b"mystery".to_vec()
        })
    );
    // The channel table keeps channel 9's frame out of this stream — on a
    // shared connection it may still belong to a channel installed later.
    assert_eq!(attach.recv().await, Some(Output::Data(b"ours".to_vec())));
    drop(attach);
    joined(server).await;
}

#[tokio::test]
async fn held_frames_replay_in_order_at_install() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(ALL_METHODS).await;
        let req = core.read_request().await;
        // Stream frames arrive BEFORE the attach reply installs the
        // channel — a foreign DATA, our RESIZE, our DATA, then the reply —
        // all in one write, all held until install.
        let mut one_write = Vec::new();
        one_write.extend_from_slice(&Frame::data(9, b"foreign".to_vec()).encode());
        one_write.extend_from_slice(&Frame::resize(4, 120, 40).encode());
        one_write.extend_from_slice(&Frame::data(4, b"held-paint".to_vec()).encode());
        one_write.extend_from_slice(
            &Frame::data(
                0,
                serde_json::to_vec(&json!({
                    "jsonrpc": "2.0", "id": req["id"],
                    "result": attach_result(4, minimal_terminal(), false, false),
                }))
                .unwrap(),
            )
            .encode(),
        );
        core.send(&one_write).await;
        core.drain_until_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    assert_eq!(attach.channel(), 4);
    // Channel 9's frame stays held for its own channel — never terminal
    // output or credit here — while channel 4's held frames replay in order.
    assert_eq!(
        attach.recv().await,
        Some(Output::Resize {
            cols: 120,
            rows: 40
        })
    );
    assert_eq!(
        attach.recv().await,
        Some(Output::Data(b"held-paint".to_vec()))
    );
    assert_eq!(attach.unconsumed(), b"held-paint".len());
    drop(attach);
    joined(server).await;
}

#[tokio::test]
async fn channel_zero_reply_is_rejected() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(0, minimal_terminal(), false, false),
        )
        .await;
        core.drain_until_eof().await;
        core
    });
    let err = tokio::time::timeout(DEADLINE, Attach::open(read, write, None, &params()))
        .await
        .expect("attach open timed out")
        .expect_err("channel 0 must be refused");
    assert!(matches!(err, AttachError::Protocol(_)), "{err:?}");
    joined(server).await;
}

#[tokio::test]
async fn a_structured_attach_error_surfaces() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(ALL_METHODS).await;
        let req = core.read_request().await;
        core.reply_error(&req["id"], "finished", "terminal t7 is finished")
            .await;
        core.drain_until_eof().await;
        core
    });
    let err = tokio::time::timeout(DEADLINE, Attach::open(read, write, None, &params()))
        .await
        .expect("attach open timed out")
        .expect_err("finished attach must fail");
    match err {
        AttachError::Call(hc_proto::client::CallError::Rpc(e)) => {
            assert!(e.is_kind("finished"), "{e:?}");
            assert_eq!(e.hint(), Some("terminal t7 is finished"));
        }
        other => panic!("expected a structured Rpc error, got {other:?}"),
    }
    joined(server).await;
}

#[tokio::test]
async fn missing_attach_method_is_unsupported() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["hello", "terminal.list"]).await;
        core.drain_until_eof().await;
        core
    });
    let err = tokio::time::timeout(DEADLINE, Attach::open(read, write, None, &params()))
        .await
        .expect("attach open timed out")
        .expect_err("no terminal.attach must fail");
    assert!(
        matches!(
            err,
            AttachError::Call(hc_proto::client::CallError::Unsupported { .. })
        ),
        "{err:?}"
    );
    joined(server).await;
}

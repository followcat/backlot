//! Attach send surface: observer refusals, input chunking, control
//! frame encodings and capability-gated kinds — all against scripted
//! fake cores. Each test waits for the server to observe the frames
//! before the attach drops, then asserts EOF separately.

mod common;

use common::attach::{DEADLINE, joined, observer, opening, params};
use common::fakecore::{ALL_METHODS, attach_result, pair};
use common::minimal_terminal;
use hc_proto::client::{AttachError, CloseReason};
use hc_proto::frame::Kind;
use hc_proto::terminal::Display;
use serde_json::{Value, json};

#[tokio::test]
async fn observer_cannot_send_anything() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(1, minimal_terminal(), true, false),
        )
        .await;
        // The only stream frame the client may send is detach's CLOSE.
        let close = core.read_frame().await;
        assert_eq!(close.kind, Kind::Close);
        assert_eq!(close.channel, 1);
        assert!(close.payload.is_empty());
        core.send_stream(1, 2, b"").await; // ack
        core.expect_eof().await;
        core
    });
    let attach = opening(read, write, &observer()).await;
    assert!(attach.observe());
    for result in [
        attach.send_input(b"ls\n").await.err(),
        attach.reply(b"auto").await.err(),
        attach.resize(120, 40).await.err(),
        attach.focus(true).await.err(),
        attach.facts(&Display::default()).await.err(),
    ] {
        assert!(matches!(result, Some(AttachError::Observe)), "{result:?}");
    }
    let ended = attach.detach().await.expect("detach failed");
    assert_eq!(ended.reason, Some(CloseReason::Empty));
    joined(server).await;
}

#[tokio::test]
async fn empty_input_writes_nothing() {
    let (mut core, read, write) = pair(64 * 1024);
    let (seen, seen_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &["automatic_input"],
            attach_result(1, minimal_terminal(), false, true),
        )
        .await;
        // The first stream frame must be the RESIZE — empty input sent
        // nothing before it.
        let frame = core.read_frame().await;
        assert_eq!(frame.kind, Kind::Resize);
        let _ = seen.send(());
        core.expect_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    attach.send_input(b"").await.expect("empty input");
    attach.reply(b"").await.expect("empty reply");
    attach.resize(80, 24).await.expect("resize");
    seen_rx.await.expect("the server never saw RESIZE");
    drop(attach);
    joined(server).await;
}

#[tokio::test]
async fn controls_encode_on_the_wire() {
    let (mut core, read, write) = pair(64 * 1024);
    let display = Display {
        bg: Some("#000011".into()),
        cell: Some([8, 16]),
        cursor: Some("#ff00ff".into()),
        fg: None,
    };
    let (seen, seen_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(5, minimal_terminal(), false, false),
        )
        .await;
        let resize = core.read_frame().await;
        assert_eq!((resize.channel, resize.kind), (5, Kind::Resize));
        assert_eq!(resize.payload, vec![120, 0, 40, 0]); // cols/rows u16 LE
        let focus = core.read_frame().await;
        assert_eq!((focus.channel, focus.kind), (5, Kind::Focus));
        assert_eq!(focus.payload, vec![1]);
        let focus = core.read_frame().await;
        assert_eq!(focus.payload, vec![0]);
        let facts = core.read_frame().await;
        assert_eq!((facts.channel, facts.kind), (5, Kind::Facts));
        let facts: Value = serde_json::from_slice(&facts.payload).unwrap();
        assert_eq!(facts["bg"], "#000011");
        assert_eq!(facts["cell"], json!([8, 16]));
        assert_eq!(facts["cursor"], "#ff00ff");
        let _ = seen.send(());
        core.expect_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    attach.resize(120, 40).await.expect("resize");
    attach.focus(true).await.expect("focus on");
    attach.focus(false).await.expect("focus off");
    attach.facts(&display).await.expect("facts");
    seen_rx.await.expect("the server never saw the frames");
    drop(attach);
    joined(server).await;
}

#[tokio::test]
async fn large_input_chunks_below_the_frame_limit() {
    let (mut core, read, write) = pair(64 * 1024);
    let (seen, seen_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(2, minimal_terminal(), false, false),
        )
        .await;
        let mut chunks = Vec::new();
        for _ in 0..3 {
            chunks.push(core.read_frame().await);
        }
        let _ = seen.send(());
        core.expect_eof().await;
        (core, chunks)
    });
    let attach = opening(read, write, &params()).await;
    let input = vec![b'x'; 40 * 1024];
    attach.send_input(&input).await.expect("large input");
    seen_rx.await.expect("the server never saw the chunks");
    drop(attach);
    let (_core, chunks) = tokio::time::timeout(DEADLINE, server)
        .await
        .expect("server timeout")
        .expect("server panic");
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].payload.len(), 16 * 1024);
    assert_eq!(chunks[1].payload.len(), 16 * 1024);
    assert_eq!(chunks[2].payload.len(), 8 * 1024);
    for frame in &chunks {
        assert_eq!((frame.channel, frame.kind), (2, Kind::Data));
    }
    let whole: Vec<u8> = chunks.into_iter().flat_map(|f| f.payload).collect();
    assert_eq!(whole, input);
}

#[tokio::test]
async fn oversized_facts_are_rejected_before_encoding() {
    let (mut core, read, write) = pair(64 * 1024);
    let (seen, seen_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(1, minimal_terminal(), false, false),
        )
        .await;
        // The wire still works: the next frame is FOCUS, not a FACTS that
        // could never encode.
        let frame = core.read_frame().await;
        assert_eq!(frame.kind, Kind::Focus);
        let _ = seen.send(());
        core.expect_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    let display = Display {
        bg: Some("x".repeat(hc_proto::frame::MAX_PAYLOAD + 1)),
        ..Display::default()
    };
    let err = attach.facts(&display).await.expect_err("too-large facts");
    assert!(
        matches!(
            err,
            AttachError::Call(hc_proto::client::CallError::TooLarge { .. })
        ),
        "{err:?}"
    );
    attach.focus(true).await.expect("wire still usable");
    seen_rx.await.expect("the server never saw FOCUS");
    drop(attach);
    joined(server).await;
}

#[tokio::test]
async fn automatic_input_needs_both_confirmations() {
    // (hello feature, reply flag) → the kind byte `reply` must use.
    for (feature, flag, expected_kind) in [
        (true, true, 6u8),
        (true, false, 0u8),
        (false, true, 0u8),
        (false, false, 0u8),
    ] {
        let (mut core, read, write) = pair(64 * 1024);
        let features: &[&str] = if feature { &["automatic_input"] } else { &[] };
        let (seen, seen_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            core.serve_attach(
                ALL_METHODS,
                features,
                attach_result(1, minimal_terminal(), false, flag),
            )
            .await;
            let frame = core.read_frame().await;
            let _ = seen.send(());
            core.expect_eof().await;
            (core, frame)
        });
        let attach = opening(read, write, &params()).await;
        assert_eq!(
            attach.automatic_input(),
            feature && flag,
            "{feature}/{flag}"
        );
        attach.reply(b"terminal-query-answer").await.expect("reply");
        seen_rx.await.expect("the server never saw the reply frame");
        drop(attach);
        let (_core, frame) = tokio::time::timeout(DEADLINE, server)
            .await
            .expect("server timeout")
            .expect("server panic");
        assert_eq!(frame.channel, 1);
        assert_eq!(frame.kind.to_u8(), expected_kind, "{feature}/{flag}");
        assert_eq!(frame.payload, b"terminal-query-answer");
    }
}

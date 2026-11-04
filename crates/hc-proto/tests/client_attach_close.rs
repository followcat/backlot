//! Attach close behavior: detach/ack, end reasons, EOF and protocol
//! failures, waiter wakeups and cancelled-open cleanup — all against
//! scripted fake cores.

mod common;

use common::attach::{DEADLINE, joined, opening, params};
use common::fakecore::{ALL_METHODS, attach_result, pair};
use common::minimal_terminal;
use hc_proto::client::{Attach, AttachError, CloseReason, Output};
use hc_proto::frame::Kind;
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn detach_sends_close_and_reports_the_ack() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(1, minimal_terminal(), false, false),
        )
        .await;
        let close = core.read_frame().await;
        assert_eq!((close.channel, close.kind), (1, Kind::Close));
        assert!(close.payload.is_empty());
        core.send_stream(1, 2, b"").await;
        core.expect_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    let ended = attach.detach().await.expect("detach failed");
    assert_eq!(ended.reason, Some(CloseReason::Empty));
    assert!(ended.transport.status.is_none() || ended.transport.status.unwrap().success());
    joined(server).await;
}

#[tokio::test]
async fn program_exit_close_preserves_its_reason() {
    let (mut core, read, write) = pair(64 * 1024);
    let reason = json!({"reason": "exited", "code": 0, "custom": {"nested": [1, 2]}});
    let wire_reason = reason.to_string();
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(1, minimal_terminal(), false, false),
        )
        .await;
        core.send_stream(1, 2, wire_reason.as_bytes()).await;
        core.drain_until_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    let event = attach.recv().await.expect("no close event");
    assert_eq!(event, Output::Closed(CloseReason::Json(reason.clone())));
    // Detach after the program ended: no resend, the stored reason reports.
    let ended = attach.detach().await.expect("detach failed");
    assert_eq!(ended.reason, Some(CloseReason::Json(reason)));
    joined(server).await;
}

#[tokio::test]
async fn a_raw_close_reason_is_preserved_byte_for_byte() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(1, minimal_terminal(), false, false),
        )
        .await;
        core.send_stream(1, 2, b"o\x01ps not json").await;
        core.drain_until_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    assert_eq!(
        attach.recv().await,
        Some(Output::Closed(CloseReason::Raw(
            b"o\x01ps not json".to_vec()
        )))
    );
    let ended = attach.detach().await.expect("detach failed");
    assert_eq!(
        ended.reason,
        Some(CloseReason::Raw(b"o\x01ps not json".to_vec()))
    );
    joined(server).await;
}

#[tokio::test]
async fn eof_fails_the_stream_and_cleanup_still_runs() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(1, minimal_terminal(), false, false),
        )
        .await;
        core // drop: the client's reader hits EOF
    });
    let attach = opening(read, write, &params()).await;
    joined(server).await;
    let event = attach.recv().await.expect("no terminal event");
    assert!(matches!(event, Output::Failed(_)), "{event:?}");
    assert_eq!(attach.recv().await, None);
    assert!(attach.is_closed());
    let ended = attach.detach().await.expect("detach on a dead wire");
    assert_eq!(ended.reason, None);
}

#[tokio::test]
async fn a_malformed_stream_frame_fails_the_attach() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(1, minimal_terminal(), false, false),
        )
        .await;
        // RESIZE must be 4 bytes; this header claims 3 — a wire violation.
        let mut raw = 1u32.to_le_bytes().to_vec();
        raw.push(3);
        raw.extend_from_slice(&3u32.to_le_bytes());
        raw.extend_from_slice(&[1, 2, 3]);
        core.send(&raw).await;
        core.drain_until_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    let event = attach.recv().await.expect("no terminal event");
    assert!(matches!(event, Output::Failed(_)), "{event:?}");
    assert!(attach.is_closed());
    drop(attach);
    joined(server).await;
}

#[tokio::test]
async fn multiple_recv_waiters_all_wake() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(1, minimal_terminal(), false, false),
        )
        .await;
        core.send_stream(1, 0, b"one").await;
        core.send_stream(1, 2, b"").await;
        core.drain_until_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    // Three waiters: each sees a change; none can be left parked forever.
    let (a, b, c) = tokio::time::timeout(DEADLINE, async {
        tokio::join!(attach.recv(), attach.recv(), attach.recv())
    })
    .await
    .expect("a waiter was left parked");
    let mut seen = vec![a, b, c];
    seen.sort_by_key(|e| format!("{e:?}"));
    assert!(
        seen.iter().any(|e| matches!(e, Some(Output::Data(_))))
            && seen.iter().any(|e| matches!(e, Some(Output::Closed(_)))),
        "{seen:?}"
    );
    drop(attach);
    joined(server).await;
}

#[tokio::test]
async fn writes_fail_after_the_stream_closed() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(1, minimal_terminal(), false, false),
        )
        .await;
        core.send_stream(1, 2, b"").await;
        core.drain_until_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    assert_eq!(
        attach.recv().await,
        Some(Output::Closed(CloseReason::Empty))
    );
    assert!(attach.is_closed());
    assert!(matches!(
        attach.send_input(b"x").await,
        Err(AttachError::Closed)
    ));
    assert!(matches!(
        attach.resize(80, 24).await,
        Err(AttachError::Closed)
    ));
    assert!(matches!(attach.focus(true).await, Err(AttachError::Closed)));
    assert!(matches!(
        attach.facts(&hc_proto::terminal::Display::default()).await,
        Err(AttachError::Closed)
    ));
    drop(attach);
    joined(server).await;
}

#[tokio::test(start_paused = true)]
async fn detach_owns_enqueue_and_ack_under_one_deadline() {
    // The peer is held on a oneshot — it never reads and never acks, so
    // the control queue backs up behind a genuinely stalled wire and the
    // CLOSE enqueue itself parks. Enqueue + ack wait share one deadline:
    // detach still completes and cleans up, bounded by an outer timeout.
    let (mut core, read, write) = pair(64);
    let (release, release_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(1, minimal_terminal(), false, false),
        )
        .await;
        let _ = release_rx.await; // hold the wire stalled until the end
        core.drain_until_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    // Prove the control queue truly filled: a send must park, not just
    // enqueue — a stalled wire frees no capacity.
    let mut filled = false;
    for _ in 0..200 {
        match tokio::time::timeout(Duration::from_millis(5), attach.focus(true)).await {
            Err(_) => {
                filled = true;
                break;
            }
            Ok(Err(_)) => break,
            _ => {}
        }
    }
    assert!(filled, "the control queue never filled on a stalled wire");
    let ended = tokio::time::timeout(DEADLINE, attach.detach())
        .await
        .expect("detach exceeded its own deadline")
        .expect("detach failed");
    assert_eq!(ended.reason, None, "no ack arrived");
    let _ = release.send(());
    joined(server).await;
}

#[tokio::test]
async fn cancelled_open_closes_both_halves_and_fails_pending() {
    // The open future is dropped while the attach RPC is unanswered —
    // before the 30s budget — and the peer must see EOF promptly.
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(ALL_METHODS).await;
        // The attach request may or may not have arrived; EOF is the claim.
        core.drain_until_eof().await;
        core
    });
    let p = params();
    let opening = Attach::open(read, write, None, &p);
    tokio::time::timeout(Duration::from_millis(100), opening)
        .await
        .expect_err("open had to outlive the RPC");
    joined(server).await;
}

//! Attach calls that never reach a reply must not wedge the shared
//! connection: a call cancelled before its request hits the wire frees
//! its id at once; a call the core never answers becomes a bounded
//! orphan whose late reply still CLOSES the channel it names; and a
//! channel's dead slot retires once the core's close arrives instead of
//! piling up for the connection's whole life.

mod common;

use common::attach::{DEADLINE, joined, params, shared, shared_once};
use common::fakecore::{ALL_METHODS, attach_result, pair};
use common::minimal_terminal;
use hc_proto::client::{AttachError, CallError, ConnectParts, Output};
use hc_proto::frame::Kind;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

/// Yields until `ready` holds, failing the test past the fixture deadline.
async fn until(ready: impl Fn() -> bool, what: &str) {
    tokio::time::timeout(DEADLINE, async {
        while !ready() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
}

/// 256 attach opens cancelled while the control queue is full: some park
/// inside `send` and never reach the wire, the rest never get answered.
/// Every registration frees its id — the next open attaches instead of
/// failing `Full` for the connection's remaining life.
#[tokio::test]
async fn cancelled_calls_on_a_full_queue_free_their_ids() {
    // A tiny transport stalls the writer after a few frames: the 64-deep
    // control queue fills, so later attach requests park inside `send`.
    let (core, read, write) = pair(2 * 1024);
    let connects = Arc::new(AtomicU64::new(0));
    let conn = shared(
        Arc::new(Mutex::new(VecDeque::from([ConnectParts {
            read,
            write,
            guard: None,
        }]))),
        Arc::clone(&connects),
    );
    let mut core = core;
    let (release, release_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        core.serve_hello(ALL_METHODS).await;
        // The anchor attach holds a claim so the connection survives the
        // storm of cancellations.
        let req = core.read_request().await;
        core.reply(
            &req["id"],
            attach_result(1, minimal_terminal(), false, false),
        )
        .await;
        // The wire stalls: nothing is read until the test releases it.
        release_rx.await.unwrap();
        // Answer every attach request that reached the wire and collect
        // every channel close the client sends back.
        let mut channels = 1u32;
        let mut closes = Vec::new();
        while let Some(frame) = core.try_read_frame().await {
            match (frame.channel, frame.kind) {
                (0, Kind::Data) => {
                    let req: Value = serde_json::from_slice(&frame.payload).unwrap();
                    if req["method"] == "terminal.attach" {
                        channels += 1;
                        core.reply(
                            &req["id"],
                            attach_result(channels, minimal_terminal(), false, false),
                        )
                        .await;
                    }
                }
                (ch, Kind::Close) => closes.push(ch),
                _ => {}
            }
        }
        (channels, closes)
    });
    let anchor = conn.attach(&params()).await.expect("anchor attach");
    let mut doomed = Vec::new();
    for _ in 0..256 {
        doomed.push(tokio::spawn({
            let conn = conn.clone();
            async move { conn.attach(&params()).await }
        }));
    }
    // Every spawned call registered its id before parking on the queue.
    until(|| conn.attach_call_count() == 256, "all calls to register").await;
    for task in doomed {
        task.abort();
    }
    // The cancellations free every registration — sent or parked.
    until(
        || conn.attach_call_count() == 0,
        "cancelled calls to release their ids",
    )
    .await;
    release.send(()).unwrap();
    let attach = conn
        .attach(&params())
        .await
        .expect("an attach after the cancelled storm must not fail Full");
    drop(attach);
    drop(anchor);
    drop(conn);
    let (channels, mut closes) = joined(server).await;
    closes.sort_unstable();
    // Every channel the storm's orphan replies named gets its CLOSE.
    // (The anchor's and the final attach's closes race the connection's
    // last-claim shutdown — the core sees EOF either way.)
    assert_eq!(closes, (2..channels).collect::<Vec<_>>());
}

/// 256 attach calls the core receives but never answers: each timed-out
/// call leaves the in-flight table, so the next open never sees `Full`.
/// The orphan bound overflowing means the core stopped answering attach
/// calls at all — the connection is declared dead and the next open
/// rebuilds it once instead of reporting `Full` forever.
#[tokio::test(start_paused = true)]
async fn unanswered_calls_orphan_then_the_connection_is_rebuilt() {
    // The transport buffers every request the stalled core never reads:
    // each attach still reaches the wire before its deadline moves the
    // registration to the orphan set.
    let (core1, read1, write1) = pair(128 * 1024);
    let (core2, read2, write2) = pair(64 * 1024);
    let connects = Arc::new(AtomicU64::new(0));
    let conn = shared(
        Arc::new(Mutex::new(VecDeque::from([
            ConnectParts {
                read: read1,
                write: write1,
                guard: None,
            },
            ConnectParts {
                read: read2,
                write: write2,
                guard: None,
            },
        ]))),
        Arc::clone(&connects),
    );
    let mut core1 = core1;
    let (hold, hold_rx) = oneshot::channel::<()>();
    let server1 = tokio::spawn(async move {
        core1.serve_hello(ALL_METHODS).await;
        let req = core1.read_request().await;
        core1
            .reply(
                &req["id"],
                attach_result(1, minimal_terminal(), false, false),
            )
            .await;
        // The stalled `machine/tN` relay case: the core reads nothing
        // more until the test lets it drain — a plain wait, so paused
        // time advancing the call deadline never trips the fixture's
        // own I/O bound.
        hold_rx.await.unwrap();
        let mut unanswered = 0usize;
        while core1.try_read_request().await.is_some() {
            unanswered += 1;
        }
        unanswered
    });
    let mut core2 = core2;
    let (go2, go2_rx) = oneshot::channel::<()>();
    let server2 = tokio::spawn(async move {
        // The rebuild's core starts no I/O — and no I/O bound — until
        // the first connection is already dead.
        go2_rx.await.unwrap();
        core2.serve_hello(ALL_METHODS).await;
        let req = core2.read_request().await;
        core2
            .reply(
                &req["id"],
                attach_result(9, minimal_terminal(), false, false),
            )
            .await;
        core2.drain_until_eof().await;
    });
    // The anchor claim keeps the first connection alive across failures.
    let anchor = conn.attach(&params()).await.expect("anchor attach");
    for _ in 0..256 {
        let err = conn
            .attach(&params())
            .await
            .expect_err("an unanswered call");
        assert!(
            matches!(err, AttachError::Call(CallError::Timeout)),
            "expected the call deadline, got {err:?}"
        );
    }
    // The orphan bound is full: the connection tears itself down rather
    // than answering `Full` — and it is the call deadline, not `Full`,
    // that this last unanswered call reports.
    let err = conn
        .attach(&params())
        .await
        .expect_err("the overflowing call");
    assert!(
        matches!(err, AttachError::Call(CallError::Timeout)),
        "a live connection must never report Full, got {err:?}"
    );
    hold.send(()).unwrap();
    go2.send(()).unwrap();
    // The next open rebuilds once — the attach works on the fresh wire.
    let attach = conn
        .attach(&params())
        .await
        .expect("the rebuilt connection attaches");
    assert_eq!(attach.channel(), 9);
    assert_eq!(
        connects.load(Ordering::SeqCst),
        2,
        "one rebuild, not a storm"
    );
    drop(attach);
    drop(anchor);
    drop(conn);
    let unanswered = joined(server1).await;
    assert_eq!(
        unanswered, 257,
        "every timed-out call still reached the wire"
    );
    joined(server2).await;
}

/// A timed-out call becomes an orphan: its late reply still names a
/// channel the core allocated, so the channel gets a CLOSE — and once
/// the core's own close lands, no dead slot lingers for it.
#[tokio::test(start_paused = true)]
async fn a_late_reply_for_an_orphan_closes_its_channel() {
    let (conn, mut core, _) = shared_once();
    let (go, go_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        core.serve_hello(ALL_METHODS).await;
        // The anchor attach holds the connection across the timed-out call.
        let req = core.read_request().await;
        core.reply(
            &req["id"],
            attach_result(5, minimal_terminal(), false, false),
        )
        .await;
        // The doomed call is read but held past its deadline.
        let doomed = core.read_request().await;
        go_rx.await.unwrap();
        // The caller is gone; the reply still lands and its channel must
        // get a CLOSE — never a leaked viewer.
        core.reply(
            &doomed["id"],
            attach_result(7, minimal_terminal(), false, false),
        )
        .await;
        let close = core.read_frame().await;
        assert_eq!(
            (close.channel, close.kind),
            (7, Kind::Close),
            "an orphan reply's channel must close"
        );
        // The core's close for the channel retires the slot for good.
        core.send_stream(7, 2, b"").await;
        core.send_stream(5, 0, b"anchor-out").await;
        core.drain_until_eof().await;
    });
    let anchor = conn.attach(&params()).await.expect("anchor attach");
    let err = conn
        .attach(&params())
        .await
        .expect_err("the held call times out");
    assert!(
        matches!(err, AttachError::Call(CallError::Timeout)),
        "{err:?}"
    );
    go.send(()).unwrap();
    assert_eq!(
        anchor.recv().await,
        Some(Output::Data(b"anchor-out".to_vec()))
    );
    assert_eq!(
        conn.channel_map_len(),
        1,
        "only the anchor's live channel may remain"
    );
    drop(anchor);
    joined(server).await;
}

/// 1 000 attach/drop cycles on one connection: each dead slot retires as
/// the core's close ack arrives, so the channel map stays bounded — and
/// a straggler for a closed channel still drops instead of reviving a
/// slot.
#[tokio::test]
async fn attach_detach_cycles_leave_the_channel_map_bounded() {
    let (conn, mut core, _) = shared_once();
    let server = tokio::spawn(async move {
        core.serve_hello(ALL_METHODS).await;
        let mut channels = 0u32;
        let mut straggler_sent = false;
        while let Some(frame) = core.try_read_frame().await {
            match (frame.channel, frame.kind) {
                (0, Kind::Data) => {
                    let req: Value = serde_json::from_slice(&frame.payload).unwrap();
                    if req["method"] == "terminal.attach" {
                        channels += 1;
                        core.reply(
                            &req["id"],
                            attach_result(channels, minimal_terminal(), false, false),
                        )
                        .await;
                    }
                }
                (ch, Kind::Close) => {
                    // The core's echo close retires the client's dead slot.
                    core.send_stream(ch, 2, b"").await;
                    if ch == 991 && !straggler_sent {
                        // A straggler for a channel whose close already
                        // completed: on the wire right behind the echo.
                        core.send_stream(991, 0, b"late").await;
                        straggler_sent = true;
                    }
                }
                _ => {}
            }
        }
        (channels, straggler_sent)
    });
    // The anchor holds a claim: dropping the last attach on a claim is
    // what tears a connection down, and the cycles run on a live one.
    let anchor = conn.attach(&params()).await.expect("anchor attach");
    for _ in 0..1000 {
        drop(conn.attach(&params()).await.expect("attach"));
    }
    // The last attach's reply lands behind every echo and the straggler —
    // the table may hold only the two live channels by then.
    let attach = conn
        .attach(&params())
        .await
        .expect("attach after the cycles");
    assert_eq!(
        conn.channel_map_len(),
        2,
        "dead channel slots piled up across the cycles"
    );
    drop(attach);
    drop(anchor);
    drop(conn);
    let (channels, straggler_sent) = joined(server).await;
    assert_eq!(channels, 1002);
    assert!(straggler_sent, "the straggler was never put on the wire");
}

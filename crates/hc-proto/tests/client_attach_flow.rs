//! Attach stream flow: credit accounting, flow-control release and
//! independence from a separate RPC connection — all against scripted
//! fake cores.

mod common;

use common::attach::{DEADLINE, joined, opening, params};
use common::fakecore::{ALL_METHODS, attach_result, pair};
use common::minimal_terminal;
use hc_proto::client::{AttachError, Client, CloseReason, Output};
use hc_proto::frame::{INITIAL_CREDIT, Kind};
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn credit_acks_only_consumed_bytes() {
    let (mut core, read, write) = pair(64 * 1024);
    let (seen, seen_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(2, minimal_terminal(), false, false),
        )
        .await;
        core.send_stream(2, 0, &[b'p'; 100]).await;
        let credit = core.read_frame().await;
        assert_eq!((credit.channel, credit.kind), (2, Kind::Credit));
        assert_eq!(
            u32::from_le_bytes(credit.payload[..4].try_into().unwrap()),
            40
        );
        let _ = seen.send(());
        core.expect_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    assert_eq!(attach.recv().await, Some(Output::Data(vec![b'p'; 100])));
    assert_eq!(attach.unconsumed(), 100);
    attach.consume(0).expect("zero consume is a no-op");
    attach.consume(40).expect("consume 40");
    assert_eq!(attach.unconsumed(), 60);
    seen_rx.await.expect("the server never saw CREDIT");
    drop(attach);
    joined(server).await;
}

#[tokio::test]
async fn partial_consumption_sums_to_the_credit_sent() {
    let (mut core, read, write) = pair(64 * 1024);
    let (seen, seen_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(2, minimal_terminal(), false, false),
        )
        .await;
        core.send_stream(2, 0, &[b'p'; 100]).await;
        // The consumes may coalesce into one CREDIT or arrive as several —
        // only the acknowledged total is contract.
        let mut credited = 0u64;
        while credited < 40 {
            let Some(f) = core.try_read_frame().await else {
                panic!("EOF before 40 bytes were credited (got {credited})");
            };
            if f.kind == Kind::Credit {
                assert_eq!(f.channel, 2);
                credited += u32::from_le_bytes(f.payload[..4].try_into().unwrap()) as u64;
            }
        }
        let _ = seen.send(());
        core.expect_eof().await;
        (core, credited)
    });
    let attach = opening(read, write, &params()).await;
    attach.recv().await;
    attach.consume(10).expect("consume 10");
    attach.consume(30).expect("consume 30");
    seen_rx.await.expect("the server never saw the CREDITs");
    drop(attach);
    let (_core, credited) = tokio::time::timeout(DEADLINE, server)
        .await
        .expect("server timeout")
        .expect("server panic");
    assert_eq!(credited, 40);
}

#[tokio::test]
async fn over_credit_is_rejected_without_losing_the_book() {
    let (mut core, read, write) = pair(64 * 1024);
    let (seen, seen_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(2, minimal_terminal(), false, false),
        )
        .await;
        core.send_stream(2, 0, &[b'p'; 100]).await;
        let credit = core.read_frame().await;
        assert_eq!(credit.kind, Kind::Credit);
        assert_eq!(
            u32::from_le_bytes(credit.payload[..4].try_into().unwrap()),
            100
        );
        let _ = seen.send(());
        core.expect_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    attach.recv().await;
    let err = attach.consume(101).expect_err("over-credit must fail");
    match err {
        AttachError::OverCredit { asked, outstanding } => {
            assert_eq!((asked, outstanding), (101, 100));
        }
        other => panic!("expected OverCredit, got {other:?}"),
    }
    assert_eq!(
        attach.unconsumed(),
        100,
        "bookkeeping survived the rejection"
    );
    attach.consume(100).expect("the rest is still owed");
    assert_eq!(attach.unconsumed(), 0);
    seen_rx.await.expect("the server never saw CREDIT");
    drop(attach);
    joined(server).await;
}

#[tokio::test]
async fn a_stalled_attach_neither_blocks_rpc_nor_hides_close() {
    let (mut core, read, write) = pair(64 * 1024);
    let (mut core2, read2, write2) = pair(64 * 1024);
    // The attach's consumer never calls recv until the end: the reader
    // must still observe a CLOSE through a full backlog, and the second
    // connection's calls must answer throughout.
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(1, minimal_terminal(), false, false),
        )
        .await;
        for _ in 0..(INITIAL_CREDIT as usize / 16384) {
            core.send_stream(1, 0, &[b'd'; 16384]).await;
        }
        core.send_stream(1, 2, b"").await;
        core.drain_until_eof().await;
        core
    });
    let server2 = tokio::spawn(async move {
        core2.serve_hello(ALL_METHODS).await;
        let req = core2.read_request().await;
        core2
            .reply(&req["id"], json!({"terminals": [], "hidden_finished": 0}))
            .await;
        core2.drain_until_eof().await;
        core2
    });
    let attach = opening(read, write, &params()).await;
    let client = tokio::time::timeout(DEADLINE, Client::open(read2, write2, None))
        .await
        .expect("client open")
        .expect("client failed");
    let list = client
        .terminal_list(hc_proto::terminal::ListParams::default())
        .await
        .expect("rpc works while the attach stalls");
    assert!(list.terminals.is_empty());
    tokio::time::timeout(DEADLINE, async {
        while !attach.is_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the stalled consumer hid CLOSE from the reader");
    // The backlog held the whole credit window; the CLOSE still arrived.
    let mut last = None;
    while let Some(event) = attach.recv().await {
        last = Some(event);
    }
    assert_eq!(last, Some(Output::Closed(CloseReason::Empty)));
    assert!(attach.is_closed());
    drop(attach);
    client.close().await.expect("client close");
    joined(server).await;
    joined(server2).await;
}

#[tokio::test]
async fn a_queued_sender_is_released_when_the_stream_ends() {
    // Fill the input queue past its byte bound so `send_input` parks,
    // then CLOSE arrives: the parked producer must be released, not wait
    // for capacity that will never come.
    let (mut core, read, write) = pair(64 * 1024);
    let (release, release_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(1, minimal_terminal(), false, false),
        )
        .await;
        // Give the sender a moment to fill the queue and park.
        tokio::time::sleep(Duration::from_millis(100)).await;
        core.send_stream(1, 2, b"").await;
        let _ = release_rx.await;
        core.drain_until_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    let big = vec![b'x'; hc_proto::frame::INITIAL_CREDIT as usize * 3];
    // The first ~256 KiB queues; the rest parks until the CLOSE releases it.
    let result = tokio::time::timeout(DEADLINE, attach.send_input(&big))
        .await
        .expect("send_input stayed parked after CLOSE");
    assert!(matches!(result, Err(AttachError::Closed)), "{result:?}");
    let _ = release.send(());
    drop(attach);
    joined(server).await;
}

#[tokio::test]
async fn a_resize_flood_is_bounded_by_the_event_count() {
    // RESIZE events never coalesce — a flood past the backlog event bound
    // is a protocol violation, not unbounded growth. The consumer only
    // starts after the flood lands, so the bound trips deterministically.
    let (mut core, read, write) = pair(64 * 1024);
    let (sent, sent_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &["resize_notifications"],
            attach_result(1, minimal_terminal(), true, false),
        )
        .await;
        for _ in 0..(1024 + 64) {
            if !core.try_send_stream(1, 3, &[80, 0, 24, 0]).await {
                break; // the client already failed the wire
            }
        }
        let _ = sent.send(());
        core.drain_until_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    sent_rx.await.expect("the flood never landed");
    let mut resizes = 0usize;
    let mut failed = None;
    while let Some(event) = attach.recv().await {
        match event {
            Output::Resize { cols: 80, rows: 24 } => resizes += 1,
            Output::Failed(why) => {
                failed = Some(why);
                break;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(resizes >= 1024, "only {resizes} RESIZE events arrived");
    assert!(
        failed.unwrap().contains("event flood"),
        "the flood must fail the stream"
    );
    assert!(attach.is_closed());
    drop(attach);
    joined(server).await;
}

#[tokio::test]
async fn a_parked_control_send_observes_the_latched_end() {
    // A control send parked on a full queue must still see an inbound
    // CLOSE — it can never wait for capacity that may never come. The
    // wire is tiny so the writer stalls quickly and the queue stays full.
    let (mut core, read, write) = pair(64);
    // The server must not read until the parked send observed the CLOSE —
    // a read would free queue capacity and the send could complete.
    let (released, released_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(1, minimal_terminal(), false, false),
        )
        .await;
        // Let the writer fill the tiny buffer and park, then CLOSE.
        tokio::time::sleep(Duration::from_millis(100)).await;
        core.send_stream(1, 2, b"").await;
        let _ = released_rx.await;
        core.drain_until_eof().await;
        core
    });
    let attach = opening(read, write, &params()).await;
    // Fill the control queue until a send parks (5 ms is generous here —
    // a stalled wire frees no capacity).
    let mut parked = false;
    for _ in 0..200 {
        match tokio::time::timeout(Duration::from_millis(5), attach.focus(true)).await {
            Err(_) => {
                parked = true;
                break;
            }
            Ok(Err(AttachError::Closed)) => break, // the CLOSE already landed
            _ => {}
        }
    }
    assert!(parked, "a control send never parked on a stalled wire");
    // This send is parked on capacity; the inbound CLOSE must release it.
    let outcome = tokio::time::timeout(DEADLINE, attach.focus(true))
        .await
        .expect("the parked send never observed the end");
    assert!(matches!(outcome, Err(AttachError::Closed)), "{outcome:?}");
    let _ = released.send(());
    drop(attach);
    joined(server).await;
}

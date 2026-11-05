//! Write fences must follow completed transport writes and wake on wire failure.

mod common;

use common::attach::{joined, opening, params};
use common::fakecore::{ALL_METHODS, attach_result, pair};
use common::minimal_terminal;
use hc_proto::client::AttachError;
use hc_proto::frame::Kind;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::oneshot;

#[tokio::test]
async fn cancelled_fence_preserves_input_and_later_close_follows_its_write() {
    let (mut core, read, write) = pair(128);
    let (resume, resumed) = oneshot::channel();
    let mut input = vec![b'x'; 32 * 1024];
    input.extend_from_slice(b"\x1b[97;1:3u");
    let expected = input.clone();
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(1, minimal_terminal(), false, false),
        )
        .await;
        resumed.await.unwrap();
        let mut received = Vec::new();
        while received.len() < expected.len() {
            let frame = core.read_frame().await;
            assert_eq!((frame.channel, frame.kind), (1, Kind::Data));
            received.extend_from_slice(&frame.payload);
        }
        assert_eq!(received, expected);
        let close = core.read_frame().await;
        assert_eq!((close.channel, close.kind), (1, Kind::Close));
        core.send_stream(1, 2, b"").await;
        core.expect_eof().await;
    });
    let attach = opening(read, write, &params()).await;
    attach.send_input(&input).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(30), attach.drain_data())
            .await
            .is_err(),
        "queue acceptance cannot complete a fence while the peer is stalled"
    );
    resume.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), attach.drain_data())
        .await
        .expect("resumed transport did not finish its input")
        .unwrap();
    attach.detach().await.unwrap();
    joined(server).await;
}

#[tokio::test]
async fn reader_eof_wakes_a_fence_even_when_the_writer_was_blocked() {
    let (mut core, read, write) = pair(128);
    let (cut, cut_requested) = oneshot::channel();
    let (release_peer, peer_released) = oneshot::channel();
    let server = tokio::spawn(async move {
        core.serve_attach(
            ALL_METHODS,
            &[],
            attach_result(1, minimal_terminal(), false, false),
        )
        .await;
        cut_requested.await.unwrap();
        let (reader, mut writer) = core.into_parts();
        writer.shutdown().await.unwrap();
        // Keep the peer's unread input side open: the client's write
        // pump cannot finish its current frame before reader EOF cancels it.
        let _ = peer_released.await;
        drop(reader);
    });
    let attach = opening(read, write, &params()).await;
    attach.send_input(&vec![b'x'; 32 * 1024]).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(30), attach.drain_data())
            .await
            .is_err()
    );
    cut.send(()).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(1), attach.drain_data()).await;
    drop(attach);
    let _ = release_peer.send(());
    joined(server).await;
    assert!(
        matches!(result, Ok(Err(AttachError::Closed))),
        "reader EOF must fail a pending fence, got {result:?}"
    );
}

//! Client lifecycle tests: unified shutdown from either half's death,
//! protocol violations, close/drop semantics, and one scratch unix socket
//! for `Client::connect`. Scripted fake cores; every wait is bounded.

mod common;

use common::fakecore;
use common::faults::{EofReads, FailWrites};
use hc_proto::client::{CallError, Client};
use hc_proto::terminal::ListParams;
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
async fn eof_fails_pending_calls() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list"]).await;
        // The peer ends the stream with a call still unanswered.
    });
    let client = client_open(read, write).await;
    let err = timeout(client.terminal_list(ListParams::default()))
        .await
        .unwrap_err();
    assert!(matches!(err, CallError::Closed), "{err:?}");
    assert!(client.is_closed());
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn a_malformed_frame_fails_pending_calls() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list"]).await;
        // A header claiming >16 MiB trips the format check on decode.
        let mut bad = [0u8; 9];
        bad[5..9].copy_from_slice(&u32::MAX.to_le_bytes());
        core.send(&bad).await;
    });
    let client = client_open(read, write).await;
    let err = timeout(client.terminal_list(ListParams::default()))
        .await
        .unwrap_err();
    assert!(matches!(err, CallError::Closed), "{err:?}");
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn malformed_data_frame_is_a_protocol_failure() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list"]).await;
        // The request stays unanswered: the malformed frame decides its fate.
        let _req = core.read_request().await;
        core.send(&hc_proto::frame::Frame::data(0, b"not json".to_vec()).encode())
            .await;
        core
    });
    let client = client_open(read, write).await;

    let err = timeout(client.terminal_list(ListParams::default()))
        .await
        .unwrap_err();
    assert!(matches!(err, CallError::Protocol(_)), "{err:?}");
    assert!(client.is_closed());
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn null_id_error_is_a_protocol_failure() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list"]).await;
        core.read_request().await;
        let payload = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": null,
            "error": {"code": -32700, "message": "parse error"},
        }))
        .unwrap();
        core.send(&hc_proto::frame::Frame::data(0, payload).encode())
            .await;
        core
    });
    let client = client_open(read, write).await;

    let err = timeout(client.terminal_list(ListParams::default()))
        .await
        .unwrap_err();
    assert!(matches!(err, CallError::Protocol(_)), "{err:?}");
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn write_failure_fails_pending_calls_promptly() {
    // Hello passes (its two writes get through); the first frame write
    // fails — while the peer's read side stays alive, so only the write
    // failure can end things. Pending calls fail now, not in 120s.
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list"]).await;
        // Part of the request frame may land before the write fails; then
        // our half drops and the peer reads EOF.
        let arrived = core.drain_until_eof().await;
        assert!(arrived <= 1, "at most the in-flight request arrives");
        core
    });
    let client = timeout(Client::open(
        read,
        Box::pin(FailWrites {
            inner: write,
            pass: 2,
        }),
        None,
    ))
    .await
    .unwrap();

    let err = timeout(client.terminal_list(ListParams::default()))
        .await
        .unwrap_err();
    assert!(matches!(err, CallError::Closed), "{err:?}");
    assert!(client.is_closed());
    let err = timeout(client.terminal_list(ListParams::default()))
        .await
        .unwrap_err();
    assert!(matches!(err, CallError::Closed), "{err:?}");
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn read_death_shuts_the_writer_down_too() {
    // The client's read side hits EOF on its own (the peer's writer died)
    // while the peer's reader stays alive — it must observe the in-flight
    // request then a clean EOF once our writer is shut down.
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list"]).await;
        let arrived = core.drain_until_eof().await;
        assert!(arrived <= 1, "at most the in-flight request arrives");
        core
    });
    let client = timeout(Client::open(Box::pin(EofReads::new(read)), write, None))
        .await
        .unwrap();

    let err = timeout(client.terminal_list(ListParams::default()))
        .await
        .unwrap_err();
    assert!(matches!(err, CallError::Closed), "{err:?}");
    assert!(client.is_closed());
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn close_fails_pending_calls_even_without_a_reader_peer() {
    // The peer answers hello then stops reading entirely.
    let (mut core, read, write) = fakecore::pair(16);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list"]).await;
        tokio::time::sleep(Duration::from_secs(60)).await; // never reads frames
        core
    });
    let client = client_open(read, write).await;

    let pending = tokio::spawn({
        let client = client.clone();
        async move { client.terminal_list(ListParams::default()).await }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let closed = timeout(client.close()).await.unwrap();
    assert!(closed.status.is_none(), "no child behind an injected wire");
    let err = pending.await.unwrap().unwrap_err();
    assert!(matches!(err, CallError::Closed), "{err:?}");
    server.abort();
}

#[tokio::test]
async fn a_live_call_is_itself_a_handle() {
    // Dropping the named handles does not end the wire while a caller's
    // own clone survives inside the spawned call: the peer's EOF — not the
    // drop — is what fails it.
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list"]).await;
        // End the peer stream after receiving the unanswered call.
        core.read_request().await;
    });
    let client = client_open(read, write).await;
    let clone = client.clone();
    let pending = tokio::spawn({
        let client = client.clone();
        async move { client.terminal_list(ListParams::default()).await }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    drop(client);
    drop(clone);
    let err = timeout(pending).await.unwrap().unwrap_err();
    assert!(matches!(err, CallError::Closed), "{err:?}");
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn the_peer_sees_eof_when_the_wire_closes() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list"]).await;
        // After the client closes, reads hit end-of-stream.
        core.expect_eof().await;
    });
    let client = client_open(read, write).await;
    timeout(client.close()).await.unwrap();
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn cancelled_close_with_a_live_clone_leaves_no_actors() {
    let (mut core, read, write) = fakecore::pair(64);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list"]).await;
        // The in-flight request may have landed; read it, then our writer
        // half's death must give the peer a clean EOF.
        let _req = core.read_request().await;
        core.expect_eof().await;
    });
    let client = client_open(read, write).await;
    let survivor = client.clone();

    let pending = tokio::spawn({
        let client = client.clone();
        async move { client.terminal_list(ListParams::default()).await }
    });
    timeout(async {
        while client.in_flight() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    // Abort close mid-shutdown: every actor was already told to die before
    // the first await, so nothing survives even though a clone lives.
    let close = tokio::spawn(client.close());
    tokio::task::yield_now().await;
    close.abort();

    let err = timeout(pending).await.unwrap().unwrap_err();
    assert!(matches!(err, CallError::Closed), "{err:?}");
    assert!(survivor.is_closed());
    let err = timeout(survivor.terminal_list(ListParams::default()))
        .await
        .unwrap_err();
    assert!(matches!(err, CallError::Closed), "{err:?}");
    // Both halves really closed: the peer's reader observed our EOF.
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn stalled_discovery_does_not_delay_last_handle_cleanup() {
    // The discovery task holds wire cells, never `Shared`: dropping the
    // last handle must shut everything down now, not after its 5s budget —
    // the peer's EOF must arrive well inside that window.
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list", "rpc.discover"]).await;
        // The raced rpc.discover request may or may not have landed before
        // the drop; at most it does, then our halves close.
        let arrived = core.drain_until_eof().await;
        assert!(arrived <= 1, "only the discover request may arrive");
        core
    });
    let client = client_open(read, write).await;
    let clone = client.clone();
    drop(client);
    drop(clone);
    // Two seconds, far inside the 5s discovery budget: an old task that
    // held `Shared` would still be parked and this would time out.
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("peer EOF within 2s — discovery must not hold Shared")
        .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn connect_over_a_real_unix_socket() {
    use hc_proto::endpoint::{Endpoint, Local};

    // A claimed-unique dir: `create_dir` never adopts a leftover that
    // could hold a stale socket the bind below would trip on.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = loop {
        let dir = std::env::temp_dir().join(format!(
            "hc-tui-client-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        match std::fs::create_dir(&dir) {
            Ok(()) => break dir,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => panic!("create scratch dir {}: {e}", dir.display()),
        }
    };
    let socket = dir.join("fake.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (cr, cw) = stream.into_split();
        let mut core = fakecore::FakeCore::new(cr, cw);
        core.serve_hello(&["terminal.list"]).await;
        let req = core.read_request().await;
        core.reply(
            &req["id"],
            json!({"terminals": [common::minimal_terminal()], "hidden_finished": 0}),
        )
        .await;
    });
    let endpoint = Endpoint::Local(Local {
        session: "test".into(),
        socket: socket.clone(),
        holder_socket: socket.with_extension("holder.sock"),
        state: dir.join("state"),
        config: dir.join("config.toml"),
    });
    let client = timeout(Client::connect(&endpoint, None)).await.unwrap();
    let list = timeout(client.terminal_list(ListParams::default()))
        .await
        .unwrap();
    assert_eq!(list.terminals.len(), 1);
    timeout(client.close()).await.unwrap();
    timeout(server).await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

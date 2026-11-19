//! Transport counters: counted halves count every read and write, a
//! connection counts once it is established, and each establishment after
//! the first is a reconnect. Scratch sockets only; nothing touches a core.

mod common;

use std::sync::Arc;

use common::attach::params;
use common::fakecore::{ALL_METHODS, attach_result, hello_result_features};
use common::minimal_terminal;
use common::scratch::TestDir;
use hc_proto::client::{AttachConn, Client};
use hc_proto::codec::{Reader, Writer};
use hc_proto::endpoint::{Endpoint, Local};
use hc_proto::frame::Frame;
use hc_proto::stats::ConnStats;
use hc_proto::transport;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

/// A fresh directory holding one test's socket, removed on drop.
struct Scratch(TestDir);

impl Scratch {
    fn new(name: &str) -> Scratch {
        Scratch(TestDir::new(&format!("hc-proto-stats-{name}")))
    }

    fn endpoint(&self) -> Endpoint {
        Endpoint::Local(Local {
            session: "hctui-test-stats".into(),
            socket: self.0.join("core.sock"),
            holder_socket: self.0.join("core.holder.sock"),
            state: self.0.to_path_buf(),
            config: self.0.join("config.toml"),
        })
    }

    fn listen(&self) -> UnixListener {
        UnixListener::bind(self.0.join("core.sock")).unwrap()
    }
}

/// Answers the plain-mode `hello` a client opens with. It reads one byte
/// at a time so nothing past the request line is consumed.
async fn answer_hello(peer: &mut UnixStream) {
    let mut line = Vec::new();
    loop {
        let byte = peer.read_u8().await.unwrap();
        if byte == b'\n' {
            break;
        }
        line.push(byte);
    }
    let request: Value = serde_json::from_slice(&line).unwrap();
    let mut reply = serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": request["id"],
        "result": hello_result_features(ALL_METHODS, &[]),
    }))
    .unwrap();
    reply.push(b'\n');
    peer.write_all(&reply).await.unwrap();
}

#[tokio::test]
async fn counted_halves_count_each_read_and_write() {
    let scratch = Scratch::new("halves");
    let listener = scratch.listen();
    let server = tokio::spawn(async move {
        let (mut peer, _) = listener.accept().await.unwrap();
        peer.write_all(b"pong\n").await.unwrap();
        let mut ping = [0u8; 5];
        peer.read_exact(&mut ping).await.unwrap();
        assert_eq!(&ping, b"ping\n");
    });
    let stats = Arc::new(ConnStats::default());
    let conn = transport::connect(&scratch.endpoint(), None).await.unwrap();
    let (mut reader, mut writer, _guard) = conn.counted(&stats).split();
    writer.write_line(b"ping").await.unwrap();
    assert_eq!(
        reader.read_line().await.unwrap().as_deref(),
        Some(&b"pong"[..])
    );
    server.await.unwrap();
    assert_eq!(reader.read_line().await.unwrap(), None);
    let snap = stats.snapshot();
    assert_eq!((snap.bytes_out, snap.bytes_in), (5, 5), "{snap:?}");
    assert_eq!((snap.reads, snap.empty_reads), (2, 1), "{snap:?}");
    assert!(snap.writes >= 1, "{snap:?}");
    assert_eq!(snap.connects, 0, "{snap:?}");
}

#[tokio::test]
async fn each_established_client_counts_and_later_ones_are_reconnects() {
    let scratch = Scratch::new("reconnect");
    let listener = scratch.listen();
    let server = tokio::spawn(async move {
        // The first attempt hangs up before its hello: that connect fails.
        let (peer, _) = listener.accept().await.unwrap();
        drop(peer);
        let mut peers = Vec::new();
        for _ in 0..2 {
            let (mut peer, _) = listener.accept().await.unwrap();
            answer_hello(&mut peer).await;
            peers.push(peer);
        }
        peers
    });
    let stats = Arc::new(ConnStats::default());
    let endpoint = scratch.endpoint();
    assert!(
        Client::connect_counted(&endpoint, None, &stats)
            .await
            .is_err()
    );
    for _ in 0..2 {
        drop(
            Client::connect_counted(&endpoint, None, &stats)
                .await
                .unwrap(),
        );
    }
    let _peers = server.await.unwrap();
    let snap = stats.snapshot();
    assert_eq!((snap.connects, snap.reconnects), (2, 1), "{snap:?}");
    assert!(snap.bytes_out > 0 && snap.bytes_in > 0, "{snap:?}");
}

#[tokio::test]
async fn a_shared_attach_connection_counts_its_connect_and_reports_channels() {
    let scratch = Scratch::new("attach");
    let listener = scratch.listen();
    let server = tokio::spawn(async move {
        let (mut peer, _) = listener.accept().await.unwrap();
        answer_hello(&mut peer).await;
        let (read, write) = peer.into_split();
        let mut reader = Reader::new(read);
        let mut writer = Writer::new(write);
        for channel in [1, 2] {
            let request = reader.read_frame().await.unwrap().unwrap();
            let request: Value = serde_json::from_slice(&request.payload).unwrap();
            let reply = json!({
                "jsonrpc": "2.0",
                "id": request["id"],
                "result": attach_result(channel, minimal_terminal(), false, false),
            });
            let frame = Frame::data(0, serde_json::to_vec(&reply).unwrap());
            writer.write_frame(&frame).await.unwrap();
        }
        // The wire stays up while the attaches live.
        (reader, writer)
    });
    let stats = Arc::new(ConnStats::default());
    let conn = AttachConn::with_stats(scratch.endpoint(), None, Arc::clone(&stats));
    let first = conn.attach(&params()).await.unwrap();
    let second = conn.attach(&params()).await.unwrap();
    let gauges = conn.gauges();
    assert_eq!(
        (
            gauges.calls_in_flight,
            gauges.channels_live,
            gauges.channels_dead
        ),
        (0, 2, 0),
        "{gauges:?}"
    );
    let snap = stats.snapshot();
    assert_eq!(snap.connects, 1, "{snap:?}");
    assert!(snap.writes >= 2 && snap.bytes_out > 0, "{snap:?}");
    assert!(snap.bytes_in > 0, "{snap:?}");
    // The first channel closed by this client waits for the core's CLOSE;
    // the second keeps the connection alive.
    drop(first);
    let gauges = conn.gauges();
    assert_eq!(
        (gauges.channels_live, gauges.channels_dead),
        (1, 1),
        "{gauges:?}"
    );
    drop(second);
    let gauges = conn.gauges();
    assert_eq!(
        (gauges.channels_live, gauges.channels_dead),
        (0, 0),
        "{gauges:?}"
    );
    let _wire = server.await.unwrap();
}

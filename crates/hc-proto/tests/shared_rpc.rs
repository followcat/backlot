//! `shared_rpc`: concurrent callers ride one counted connect on one
//! live wire, a dead wire is rebuilt once for the next callers, and a
//! failed connect retries instead of latching. In-memory duplex wires
//! only — no real ssh, no core.
use hc_proto::client::{Client, Error, shared_rpc_with};
use hc_proto::codec::Reader;
use hc_proto::watch::OpenRpc;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncWriteExt, DuplexStream, ReadHalf as IoReadHalf, WriteHalf as IoWriteHalf};

/// A scripted fake core: answers the framed `hello`, then drains
/// frames until the client hangs up — the wire stays live meanwhile.
struct FakeCore {
    reader: Reader<IoReadHalf<DuplexStream>>,
    out: IoWriteHalf<DuplexStream>,
}

impl FakeCore {
    async fn serve(mut self) {
        let line = self.reader.read_line().await.unwrap().unwrap();
        let req: Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(req["method"], "hello");
        let mut reply = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": req["id"],
            "result": {
                "generation": 1, "version": "fake-core 0.0", "framed": true,
                "core": {"id": "core-fake-1", "name": "fake-core"},
                "methods": ["doc.get"], "features": [],
            }
        }))
        .unwrap();
        reply.push(b'\n');
        self.out.write_all(&reply).await.unwrap();
        while let Ok(Some(_)) = self.reader.read_frame().await {}
    }
}

/// One fresh in-memory wire with its fake core spawned.
async fn fake_client() -> Result<Client, Error> {
    let (server, client_side) = tokio::io::duplex(64 * 1024);
    let (cr, cw) = tokio::io::split(server);
    let (read, write) = tokio::io::split(client_side);
    tokio::spawn(
        FakeCore {
            reader: Reader::new(cr),
            out: cw,
        }
        .serve(),
    );
    Client::open(Box::pin(read), Box::pin(write), None).await
}

/// An injected connect that counts attempts; every attempt opens a
/// fresh in-memory wire served by [`FakeCore`].
fn counted(connects: &Arc<AtomicU64>) -> OpenRpc {
    let connects = Arc::clone(connects);
    shared_rpc_with(move || {
        let connects = Arc::clone(&connects);
        async move {
            connects.fetch_add(1, Ordering::SeqCst);
            fake_client().await
        }
    })
}

/// Three callers racing the first open — watcher, store and seen —
/// invoke the connect once and all share the one live wire.
#[tokio::test]
async fn concurrent_openers_share_one_connect_and_one_wire() {
    let connects = Arc::new(AtomicU64::new(0));
    let rpc = counted(&connects);
    let (a, b, c) = tokio::join!(rpc(), rpc(), rpc());
    let (a, b, c) = (a.unwrap(), b.unwrap(), c.unwrap());
    assert_eq!(
        connects.load(Ordering::SeqCst),
        1,
        "three opens, one connect"
    );
    for client in [&a, &b, &c] {
        assert!(!client.is_closed());
    }
    a.close().await.unwrap();
    assert!(b.is_closed() && c.is_closed(), "all clones ride one wire");
}

/// Once the wire dies the next opens rebuild exactly once — the
/// burst of recovering consumers still shares a single reconnect.
#[tokio::test]
async fn a_dead_wire_rebuilds_once_on_the_next_open() {
    let connects = Arc::new(AtomicU64::new(0));
    let rpc = counted(&connects);
    let first = rpc().await.unwrap();
    let keep = first.clone();
    assert!(!first.is_closed());
    first.close().await.unwrap();
    assert!(keep.is_closed());
    let (a, b) = tokio::join!(rpc(), rpc());
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(connects.load(Ordering::SeqCst), 2, "one rebuild, not two");
    assert!(!a.is_closed() && !b.is_closed());
}

/// A failed connect clears to `Empty` instead of latching — the
/// next call retries cleanly on a fresh attempt.
#[tokio::test]
async fn a_failed_connect_retries_instead_of_latching() {
    let attempts = Arc::new(AtomicU64::new(0));
    let rpc = shared_rpc_with({
        let attempts = Arc::clone(&attempts);
        move || {
            let attempts = Arc::clone(&attempts);
            async move {
                if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(Error::Closed);
                }
                fake_client().await
            }
        }
    });
    assert!(rpc().await.is_err());
    let client = rpc().await.expect("the retry connects");
    assert!(!client.is_closed());
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}

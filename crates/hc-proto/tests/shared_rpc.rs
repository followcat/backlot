//! `shared_rpc`: concurrent callers ride one counted connect on one
//! live wire, a dead wire is rebuilt once for the next callers, and a
//! failed connect retries instead of latching. In-memory duplex wires
//! only — no real ssh, no core.
use hc_proto::client::{CallError, Client, Error, shared_rpc_with};
use hc_proto::codec::Reader;
use hc_proto::frame::Frame;
use hc_proto::watch::OpenRpc;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncWriteExt, DuplexStream, ReadHalf as IoReadHalf, WriteHalf as IoWriteHalf};
use tokio::sync::mpsc;

/// A scripted fake core: answers the framed `hello`, then drains
/// frames until the client hangs up — the wire stays live meanwhile.
struct FakeCore {
    reader: Reader<IoReadHalf<DuplexStream>>,
    out: IoWriteHalf<DuplexStream>,
}

impl FakeCore {
    async fn hello(&mut self) {
        let line = self.reader.read_line().await.unwrap().unwrap();
        let req: Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(req["method"], "hello");
        let mut reply = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": req["id"],
            "result": {
                "generation": 1, "version": "fake-core 0.0", "framed": true,
                "core": {"id": "core-fake-1", "name": "fake-core"},
                "methods": ["doc.get", "machine.list"], "features": [],
            }
        }))
        .unwrap();
        reply.push(b'\n');
        self.out.write_all(&reply).await.unwrap();
    }

    /// The dead wire: hello completes, then every request is read and
    /// never answered — open, silent, exactly a blackholed path.
    async fn serve(mut self) {
        self.hello().await;
        while let Ok(Some(_)) = self.reader.read_frame().await {}
    }

    /// The slow-but-alive wire: requests log their method to `seen` —
    /// `machine.list` (the liveness probe) gets an answer while every
    /// other call parks like a slow reply that never lands.
    async fn serve_alive(mut self, seen: mpsc::Sender<String>) {
        self.hello().await;
        while let Ok(Some(frame)) = self.reader.read_frame().await {
            if frame.channel != 0 {
                continue;
            }
            let req: Value = serde_json::from_slice(&frame.payload).unwrap();
            let method = req["method"].as_str().unwrap().to_string();
            let _ = seen.send(method.clone()).await;
            if method == "machine.list" {
                let payload = serde_json::to_vec(&json!({
                    "jsonrpc": "2.0", "id": req["id"], "result": {"machines": []},
                }))
                .unwrap();
                self.out
                    .write_all(&Frame::data(0, payload).encode())
                    .await
                    .unwrap();
            }
        }
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

/// A wire that stays open but never answers — the fake drains every
/// request — leaves `is_closed` false, so a call that times out would
/// be retried onto the same dead wire forever. After the timeout one
/// liveness probe runs on the same wire; its silence closes the client
/// and the next `open_rpc` connects fresh.
#[tokio::test(start_paused = true)]
async fn a_silent_wire_is_probed_then_replaced() {
    let connects = Arc::new(AtomicU64::new(0));
    let rpc = counted(&connects);
    let client = rpc().await.unwrap();

    let timed_out = tokio::time::timeout(
        Duration::from_secs(60),
        client.call_timeout("doc.get", None, Duration::from_millis(50)),
    )
    .await
    .expect("the call had to finish")
    .expect_err("a silent wire cannot answer");
    assert!(matches!(timed_out, CallError::Timeout), "{timed_out:?}");

    // The probe's own short deadline passes with nothing back: the
    // wire is declared dead — never on the first timeout alone. `sleep`
    // parks this task so auto-advance can schedule the spawned probe
    // and fire its five-second deadline; `advance` only walks timers.
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert!(
        client.is_closed(),
        "a probe answered by silence had to close the wire"
    );

    let second = rpc().await.unwrap();
    assert!(!second.is_closed());
    assert_eq!(
        connects.load(Ordering::SeqCst),
        2,
        "a dead wire reconnects on the next open"
    );
}

/// The first timeout alone must never kill a live wire: the fake parks
/// `doc.get` but answers the `machine.list` probe — proof of life on
/// the same wire — so the shared client stays and keeps being reused.
#[tokio::test(start_paused = true)]
async fn a_slow_but_alive_wire_keeps_its_connection() {
    let (seen_tx, mut seen) = mpsc::channel::<String>(16);
    let connects = Arc::new(AtomicU64::new(0));
    let rpc = shared_rpc_with({
        let connects = Arc::clone(&connects);
        move || {
            let connects = Arc::clone(&connects);
            let seen_tx = seen_tx.clone();
            async move {
                connects.fetch_add(1, Ordering::SeqCst);
                let (server, client_side) = tokio::io::duplex(64 * 1024);
                let (cr, cw) = tokio::io::split(server);
                let (read, write) = tokio::io::split(client_side);
                tokio::spawn(
                    FakeCore {
                        reader: Reader::new(cr),
                        out: cw,
                    }
                    .serve_alive(seen_tx),
                );
                Client::open(Box::pin(read), Box::pin(write), None).await
            }
        }
    });
    let client = rpc().await.unwrap();

    let timed_out = tokio::time::timeout(
        Duration::from_secs(60),
        client.call_timeout("doc.get", None, Duration::from_millis(50)),
    )
    .await
    .expect("the call had to finish")
    .expect_err("a parked call cannot answer");
    assert!(matches!(timed_out, CallError::Timeout), "{timed_out:?}");

    // The timeout's liveness probe reaches the live core and is
    // answered: the wire survives and stays the shared connection.
    let probed = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let method = seen.recv().await.unwrap();
            if method == "machine.list" {
                break method;
            }
        }
    })
    .await
    .expect("the liveness probe never reached the core");
    assert_eq!(probed, "machine.list");
    assert!(!client.is_closed(), "an answered probe keeps the wire");

    let same = rpc().await.unwrap();
    assert!(!same.is_closed());
    assert_eq!(
        connects.load(Ordering::SeqCst),
        1,
        "a live wire is never rebuilt on a timeout alone"
    );
}

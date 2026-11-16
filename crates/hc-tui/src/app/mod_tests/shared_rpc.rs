//! The cli wiring's shared RPC: the watcher's refreshes, the layout
//! store's actions and the seen worker's label writes ride one framed
//! connection — and one rebuild when the wire dies, never a failed
//! call first.
use super::*;
use crate::operations::{Action, MachineOp, Outcome, Request, Store};
use crate::testcore::{self, FakeCore};
use hc_proto::client::Client;
use hc_proto::frame::Kind;
use hc_proto::watch::{Link, OpenRpc, WatchParams};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;

/// The counted shared opener `cli::run` builds over the endpoint: each
/// connect gets a fresh duplex pair and pushes its server half to
/// `accepted`.
fn counted_rpc(accepted: mpsc::Sender<FakeCore>, connects: Arc<AtomicU64>) -> OpenRpc {
    hc_proto::client::shared_rpc_with(move || {
        let tx = accepted.clone();
        let connects = Arc::clone(&connects);
        async move {
            connects.fetch_add(1, Ordering::SeqCst);
            let (core, read, write) = testcore::pair(64 * 1024);
            if tx.send(core).await.is_err() {
                return Err(hc_proto::client::Error::Timeout("fake accept"));
            }
            Client::open(read, write, None).await
        }
    })
}

/// An app whose watcher refreshes ride `rpc` — the same opener
/// `set_operations` gets, exactly the way `cli::run` wires them. The
/// subscription keeps its own dedicated conn on `sub_rx`.
fn shared_app(rpc: OpenRpc) -> (App, mpsc::Receiver<FakeCore>) {
    let (sub_accepted, sub_rx) = mpsc::channel(4);
    let (_, sub) = testcore::openers(sub_accepted);
    let watcher = Watcher::with_openers(
        rpc,
        sub,
        WatchParams {
            traversal: Some(crate::project::agent_traversal),
            ..WatchParams::default()
        },
    );
    let open: OpenAttach = Arc::new(|_| Box::pin(std::future::pending()));
    let config = crate::config::Config::default();
    let live = config.live_keybinds();
    let shell = crate::chrome::ClientShellConfig::from_config(
        &config,
        None,
        None,
        std::path::PathBuf::new(),
    );
    let mut app = App::new(watcher, open, live, shell, Arc::new(Probe::default()));
    app.set_plain(true);
    (app, sub_rx)
}

/// Serves one accepted RPC conn: framed hello, then answers every
/// channel-0 request by method until the wire dies. `terminal.label`
/// params are reported on `labels` so the test can tell which conn a
/// seen write rode.
async fn serve(mut core: FakeCore, labels: mpsc::Sender<Value>) {
    core.read_hello().await;
    core.answer_hello(
        true,
        &[
            "doc.get",
            "doc.set",
            "machine.list",
            "machine.connect",
            "terminal.list",
            "terminal.label",
        ],
        &[],
    )
    .await;
    let mut version = 0u64;
    while let Some(frame) = core.try_read_frame().await {
        if frame.channel != 0 || frame.kind != Kind::Data {
            continue;
        }
        let req: Value = serde_json::from_slice(&frame.payload).unwrap();
        if req["method"] == "terminal.label" {
            let _ = labels.send(req["params"].clone()).await;
        }
        let result = match req["method"].as_str() {
            Some("doc.get") => json!({"value": null, "version": 1}),
            Some("doc.set") => {
                version += 1;
                json!({"version": version})
            }
            Some("machine.list") => json!({"machines": []}),
            Some("terminal.list") => json!({"terminals": []}),
            _ => json!({}),
        };
        core.reply(&req["id"], result).await;
    }
}

/// The store's next result, unwrapped of the timeout envelope.
async fn settle(app: &mut App) -> Option<Result<Outcome, String>> {
    tokio::time::timeout(
        testcore::DEADLINE,
        App::operation_result(&mut app.operations),
    )
    .await
    .expect("action never resolved")
}

/// The seen worker's next result, unwrapped of the timeout envelope.
async fn seen_result(app: &mut App) -> super::seen::SeenResult {
    tokio::time::timeout(testcore::DEADLINE, app.seen_rx.recv())
        .await
        .expect("seen write never resolved")
        .expect("seen worker stopped")
}

fn connect_host_b() -> Action {
    Action::Machine(MachineOp::Connect {
        machine: "host-b".into(),
    })
}

#[tokio::test]
async fn watcher_store_and_seen_share_one_rpc_which_rebuilds_once() {
    let connects = Arc::new(AtomicU64::new(0));
    let (rpc_accepted, mut rpc_rx) = mpsc::channel(8);
    let rpc = counted_rpc(rpc_accepted, Arc::clone(&connects));
    // A fourth consumer's clone drives the wire close below.
    let first = tokio::spawn(rpc());
    let (label_tx1, mut label_rx1) = mpsc::channel(4);
    let serve1 = tokio::spawn(serve(rpc_rx.recv().await.unwrap(), label_tx1));
    let handle = first.await.unwrap().unwrap();
    let (mut app, mut sub_rx) = shared_app(rpc.clone());
    app.set_operations(rpc);

    // The dedicated subscription publishes t1's snapshot on its own
    // conn; refreshes ride the shared framed one.
    let mut t1 = testcore::minimal_terminal();
    t1["address"] = json!("t1");
    t1["labels"] = json!({"herdr.space": "space-a", "herdr.tab": "tab-a"});
    let sub_server = tokio::spawn(async move {
        let mut core = sub_rx.recv().await.unwrap();
        core.read_hello().await;
        testcore::serve_subscribe(&mut core, json!({"seq": 1, "terminals": [t1]})).await;
        std::future::pending::<()>().await;
    });
    testcore::await_view(&app.watcher, |view| {
        matches!(view.link, Link::Online) && matches!(view.rpc, Link::Online)
    })
    .await;
    assert_eq!(
        connects.load(Ordering::SeqCst),
        1,
        "watcher used the shared wire"
    );

    publish_agent(&mut app, "t1", 5);

    // A store action lands on the same conn — not a new one.
    app.submit_action(connect_host_b());
    let result = settle(&mut app).await;
    assert!(
        matches!(&result, Some(Ok(Outcome::Done))),
        "action failed: {:?}",
        result.as_ref().map(|r| r.as_ref().map(|_| ()))
    );
    app.on_operation(result);
    assert_eq!(
        connects.load(Ordering::SeqCst),
        1,
        "store opened its own conn"
    );

    // So does the seen worker's label write.
    app.share_seen();
    let label = tokio::time::timeout(testcore::DEADLINE, label_rx1.recv())
        .await
        .expect("seen write never reached the core")
        .unwrap();
    assert_eq!(
        label,
        json!({"terminal": "t1", "labels": {"herdr.seen": "5"}})
    );
    let result = seen_result(&mut app).await;
    assert!(
        result.result.is_ok(),
        "seen write failed: {:?}",
        result.result
    );
    app.on_seen_result(result);
    assert_eq!(
        connects.load(Ordering::SeqCst),
        1,
        "seen opened its own conn"
    );

    // The wire dies: every clone's `is_closed` latches at once.
    handle.close().await.unwrap();
    drop(serve1);

    // The cached dead client is detected, not called: the very next
    // action rebuilds once and succeeds on the first try.
    app.submit_action(connect_host_b());
    let (label_tx2, mut label_rx2) = mpsc::channel(4);
    let serve2 = tokio::spawn(serve(
        tokio::time::timeout(testcore::DEADLINE, rpc_rx.recv())
            .await
            .expect("no rebuild after the wire died")
            .unwrap(),
        label_tx2,
    ));
    let result = settle(&mut app).await;
    assert!(
        matches!(&result, Some(Ok(Outcome::Done))),
        "first action after the death failed: {:?}",
        result.as_ref().map(|r| r.as_ref().map(|_| ()))
    );
    app.on_operation(result);
    assert_eq!(
        connects.load(Ordering::SeqCst),
        2,
        "the rebuild was not single-flight"
    );

    // The seen worker detects the dead wire the same way: this turn's
    // write lands on conn 2 on the first try.
    publish_agent(&mut app, "t1", 6);
    app.share_seen();
    let label = tokio::time::timeout(testcore::DEADLINE, label_rx2.recv())
        .await
        .expect("seen write did not ride the rebuilt conn")
        .unwrap();
    assert_eq!(
        label,
        json!({"terminal": "t1", "labels": {"herdr.seen": "6"}})
    );
    let result = seen_result(&mut app).await;
    assert!(
        result.result.is_ok(),
        "first seen write after the death failed: {:?}",
        result.result
    );
    app.on_seen_result(result);
    assert_eq!(connects.load(Ordering::SeqCst), 2);

    sub_server.abort();
    drop(serve2);
}

/// A dead-but-cached client is detected before the next action, never
/// called: the store reconnects and the action succeeds on the first
/// try. `built` hands the test the very `Client` the store cached so
/// the wire's death is awaited, not guessed.
#[tokio::test]
async fn a_dead_cached_client_is_detected_not_called() {
    let (core1, read1, write1) = testcore::pair(64 * 1024);
    let (core2, read2, write2) = testcore::pair(64 * 1024);
    let wires = std::sync::Mutex::new(VecDeque::from([(read1, write1), (read2, write2)]));
    let (built_tx, mut built_rx) = mpsc::channel(4);
    let open: OpenRpc = Arc::new(move || {
        let (read, write) = wires.lock().unwrap().pop_front().expect("one conn");
        let built_tx = built_tx.clone();
        Box::pin(async move {
            let client = Client::open(read, write, None).await?;
            let _ = built_tx.send(client.clone()).await;
            Ok(client)
        })
    });
    let mut store = Store::new(open);
    let request = || Request {
        core: "core-fake-1".into(),
        action: connect_host_b(),
    };
    let server1 = tokio::spawn(async move {
        let mut core = core1;
        core.read_hello().await;
        core.answer_hello(true, &["machine.connect"], &[]).await;
        let req = core.read_request().await;
        core.reply(&req["id"], json!({})).await;
        std::future::pending::<()>().await;
        core
    });
    store.submit(request()).unwrap();
    assert!(
        matches!(
            tokio::time::timeout(testcore::DEADLINE, store.results.recv())
                .await
                .unwrap()
                .unwrap(),
            Ok(Outcome::Done)
        ),
        "warmup action failed"
    );
    let cached = built_rx.recv().await.unwrap();
    assert!(!cached.is_closed());
    // The wire dies under the cached client: kill the core end and wait
    // for the client's own dead latch — the store cannot still be
    // holding a live wire by accident.
    server1.abort();
    tokio::time::timeout(testcore::DEADLINE, cached.closed())
        .await
        .expect("the dead latch never landed");
    store.submit(request()).unwrap();
    let server2 = tokio::spawn(async move {
        let mut core = core2;
        core.read_hello().await;
        core.answer_hello(true, &["machine.connect"], &[]).await;
        let req = core.read_request().await;
        core.reply(&req["id"], json!({})).await;
    });
    assert!(
        matches!(
            tokio::time::timeout(testcore::DEADLINE, store.results.recv())
                .await
                .unwrap()
                .unwrap(),
            Ok(Outcome::Done)
        ),
        "the first action after the death failed"
    );
    server2.await.unwrap();
}

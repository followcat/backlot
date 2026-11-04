//! Watcher test scaffolding: openers that give every connect attempt a
//! fresh duplex pair (the server half goes to the test's `accepted`
//! queue), a fake core dispatching each accepted conn by its hello
//! `framed` flag, and bounded view assertions.
#![allow(dead_code)]

use super::fakecore::{self, DuplexCore};
use hc_proto::client::{CallError, Client};
use hc_proto::events::SubscribeParams;
use hc_proto::subscribe::{SubscribeError, Subscription};
use hc_proto::watch::{BoxFuture, OpenRpc, OpenSub, SubAttempt, View, Watcher};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Opener closures over fresh `duplex` pairs: each call pushes the
/// server half to `accepted` and opens the client half, the way a real
/// `connect` re-establishes a transport per attempt.
pub fn openers(accepted: mpsc::Sender<DuplexCore>) -> (OpenRpc, OpenSub) {
    let tx = accepted.clone();
    let rpc: OpenRpc = Arc::new(move || {
        let tx = tx.clone();
        Box::pin(async move {
            let (core, read, write) = fakecore::pair(64 * 1024);
            if tx.send(core).await.is_err() {
                return Err(hc_proto::client::Error::Timeout("fake accept"));
            }
            Client::open(read, write, None).await
        })
    });
    let sub: OpenSub = Arc::new(move |params: SubscribeParams| {
        let tx = accepted.clone();
        Box::pin(async move {
            let (core, read, write) = fakecore::pair(64 * 1024);
            if tx.send(core).await.is_err() {
                return SubAttempt::Failed(SubscribeError::Open(hc_proto::client::Error::Timeout(
                    "fake accept",
                )));
            }
            match Subscription::open(read, write, None, &params).await {
                Ok(subscription) => SubAttempt::Subscribed(Box::new(subscription)),
                Err(SubscribeError::Call(CallError::Unsupported { reason, .. })) => {
                    SubAttempt::Unsupported(reason)
                }
                Err(e) => SubAttempt::Failed(e),
            }
        })
    });
    (rpc, sub)
}

/// A watcher plus its accept queue: `(watcher, accepted)`. The test
/// serves every accepted conn through [`spawn_fake`].
pub fn watcher(documents: Vec<String>) -> (Watcher, mpsc::Receiver<DuplexCore>) {
    watcher_with(documents, reconnect_params())
}

/// `watcher` with caller-chosen params (small backoffs keep reconnect
/// tests fast).
pub fn watcher_with(
    documents: Vec<String>,
    mut params: hc_proto::watch::WatchParams,
) -> (Watcher, mpsc::Receiver<DuplexCore>) {
    let (accepted, rx) = mpsc::channel(16);
    let (rpc, sub) = openers(accepted);
    params.documents = documents;
    (Watcher::with_openers(rpc, sub, params), rx)
}

/// Reconnect-test tuning: the same capped backoff shape, sped up.
pub fn reconnect_params() -> hc_proto::watch::WatchParams {
    hc_proto::watch::WatchParams {
        reconnect_initial: std::time::Duration::from_millis(20),
        reconnect_max: std::time::Duration::from_millis(200),
        ..Default::default()
    }
}

/// Serves every accepted conn: `serve` gets each one with its hello
/// already read, classified `true` for framed (the RPC client) and
/// `false` for plain (the subscription).
pub fn spawn_fake(
    mut accepted: mpsc::Receiver<DuplexCore>,
    mut serve: impl FnMut(DuplexCore, bool) -> BoxFuture<()> + Send + Sync + 'static,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(mut core) = accepted.recv().await {
            let hello = core.read_hello().await;
            // The call itself is synchronous — only the returned 'static
            // future enters a task, so `serve` can stay `FnMut`.
            let work = serve(core, hello["params"]["framed"] == true);
            tokio::spawn(work);
        }
    })
}

/// The plain-conn script's head: plain hello, then the
/// `events.subscribe` request answered with `snapshot` (`{"seq":…,
/// "terminals":[…]}`). Returns the request for param assertions; the
/// caller then drives notifications through the same `core`.
pub async fn serve_subscribe(
    core: &mut DuplexCore,
    methods: &[&str],
    snapshot: serde_json::Value,
) -> serde_json::Value {
    // `spawn_fake` already read the hello — answer it, don't re-read.
    core.answer_hello(fakecore::hello_result_framed(methods, &[], false))
        .await;
    let req = core.read_message().await;
    assert_eq!(req["method"], "events.subscribe");
    core.reply_line(&req["id"], snapshot).await;
    req
}

/// A framed-conn serve loop: hello, then requests answered through
/// `answer` — `Some(Ok(result))` replies, `Some(Err((kind, hint)))`
/// replies an RPC error, `None` leaves the request deliberately
/// unanswered (a parked fetch). Ends when the client hangs up.
pub async fn serve_rpc(
    mut core: DuplexCore,
    methods: &[&str],
    mut answer: impl FnMut(&serde_json::Value) -> Option<Result<serde_json::Value, (String, String)>>
    + Send,
) {
    // `spawn_fake` already read the hello — answer it, don't re-read.
    core.answer_hello(fakecore::hello_result_framed(methods, &[], true))
        .await;
    while let Some(req) = core.try_read_request().await {
        match answer(&req) {
            Some(Ok(result)) => core.reply(&req["id"], result).await,
            Some(Err((kind, hint))) => core.reply_error(&req["id"], &kind, &hint).await,
            None => {}
        }
    }
}

/// Waits under a deadline for a view satisfying `pred` and returns it.
/// The `version`/`changed_since` cursor loses no publish between read
/// and wait.
pub async fn await_view(watcher: &Watcher, pred: impl Fn(&View) -> bool) -> View {
    tokio::time::timeout(super::attach::DEADLINE, async {
        loop {
            let view = watcher.view();
            if pred(&view) {
                return view;
            }
            let seen = view.version;
            watcher.changed_since(seen).await;
        }
    })
    .await
    .expect("timed out waiting for a view")
}

/// A `MachineView` wire record under `name`.
pub fn machine(name: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name, "target": "alice@host", "id": "core-b",
        "host": name, "version": "0.9", "state": "connected",
    })
}

/// An rpc conn: `doc.get`s whose params match `hold` park (forwarded to
/// `held`, replied later via `replies`); `machine.list` answers from the
/// shared list; everything else replies inline.
pub async fn serve_held(
    mut core: DuplexCore,
    held: mpsc::Sender<Value>,
    mut replies: mpsc::Receiver<(Value, Value)>,
    machines: Arc<Mutex<Vec<Value>>>,
    hold: &(dyn Fn(&Value) -> bool + Send + Sync),
) {
    core.answer_hello(fakecore::hello_result_framed(METHODS, &[], true))
        .await;
    loop {
        tokio::select! {
            req = core.try_read_request() => match req {
                Some(req) => match req["method"].as_str().unwrap() {
                    "doc.get" if hold(&req["params"]) => {
                        held.send(req).await.ok();
                    }
                    "doc.get" => {
                        core.reply(&req["id"], json!({"value": {"local": 1}, "version": 1}))
                            .await;
                    }
                    "machine.list" => {
                        let machines = machines.lock().unwrap().clone();
                        core.reply(&req["id"], json!({"machines": machines})).await;
                    }
                    _ => core.reply(&req["id"], Value::Null).await,
                },
                None => break,
            },
            Some((id, result)) = replies.recv() => {
                core.reply(&id, result).await;
            }
        }
    }
}

/// Fixture: the plain conn serves an empty snapshot; the rpc conn runs
/// `serve_held`. Returns everything the test drives.
#[allow(clippy::type_complexity)]
pub fn held_fixture(
    documents: Vec<String>,
    initial_machines: Vec<Value>,
    hold: Arc<dyn Fn(&Value) -> bool + Send + Sync>,
) -> (
    hc_proto::watch::Watcher,
    mpsc::Receiver<Value>,
    mpsc::Sender<(Value, Value)>,
    mpsc::Sender<(String, Value)>,
    Arc<Mutex<Vec<Value>>>,
    tokio::task::JoinHandle<()>,
) {
    let (watcher, accepted) = watcher(documents);
    let (held_tx, held) = mpsc::channel(16);
    let (replies_tx, replies_rx) = mpsc::channel(16);
    let (events_tx, events_rx) = mpsc::channel::<(String, Value)>(16);
    let machines = Arc::new(Mutex::new(initial_machines));
    let mut replies_rx = Some(replies_rx);
    let mut events_rx = Some(events_rx);
    let fake = {
        let machines = Arc::clone(&machines);
        spawn_fake(accepted, move |core, framed| {
            if framed {
                let held_tx = held_tx.clone();
                let machines = Arc::clone(&machines);
                let hold = Arc::clone(&hold);
                let replies = replies_rx.take().expect("one rpc conn");
                Box::pin(async move {
                    serve_held(core, held_tx, replies, machines, &*hold).await;
                })
            } else {
                let mut events_rx = events_rx.take().expect("one sub conn");
                Box::pin(async move {
                    let mut core = core;
                    serve_subscribe(&mut core, METHODS, json!({"seq": 1, "terminals": []})).await;
                    while let Some((method, params)) = events_rx.recv().await {
                        core.notify(&method, params).await;
                    }
                })
            }
        })
    };
    (watcher, held, replies_tx, events_tx, machines, fake)
}

/// Parks every machine doc.get (`on` set); local fetches answer inline.
pub fn hold_machines() -> Arc<dyn Fn(&Value) -> bool + Send + Sync> {
    Arc::new(|params| !params["on"].is_null())
}

/// Replies to a held request.
pub fn reply(req: &Value, result: Value) -> (Value, Value) {
    (req["id"].clone(), result)
}

/// Method lists both conns advertise in the watcher fakes.
pub const METHODS: &[&str] = &[
    "hello",
    "terminal.list",
    "machine.list",
    "doc.get",
    "events.subscribe",
];

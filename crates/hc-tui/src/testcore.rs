//! Test-only fake core: speaks `hello`, then channel-0 DATA frames plus
//! stream frames. Mirrors the hc-proto `tests/common` scaffold, trimmed to
//! what the runtime tests drive. Not every test uses every helper.
#![allow(dead_code)]

use hc_proto::client::{CallError, Client};
use hc_proto::codec::Reader;
use hc_proto::events::SubscribeParams;
use hc_proto::frame::{Frame, Kind};
use hc_proto::subscribe::{SubscribeError, Subscription};
use hc_proto::watch::{BoxFuture, OpenRpc, OpenSub, SubAttempt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncWriteExt, DuplexStream, ReadHalf as IoReadHalf, WriteHalf as IoWriteHalf};
use tokio::sync::mpsc;

pub const DEADLINE: Duration = Duration::from_secs(15);

/// Methods both fake conns advertise.
pub const METHODS: &[&str] = &[
    "hello",
    "terminal.list",
    "terminal.attach",
    "machine.list",
    "doc.get",
    "events.subscribe",
];

/// The fake end of a client wire.
pub struct FakeCore {
    reader: Reader<IoReadHalf<DuplexStream>>,
    out: IoWriteHalf<DuplexStream>,
}

/// A connected `(fake core, client read, client write)` triple.
pub fn pair(
    capacity: usize,
) -> (
    FakeCore,
    hc_proto::transport::ReadHalf,
    hc_proto::transport::WriteHalf,
) {
    let (core_side, client_side) = tokio::io::duplex(capacity);
    let (cr, cw) = tokio::io::split(core_side);
    let (read, write) = tokio::io::split(client_side);
    (
        FakeCore {
            reader: Reader::new(cr),
            out: cw,
        },
        Box::pin(read),
        Box::pin(write),
    )
}

async fn bounded<F: std::future::Future>(f: F) -> F::Output {
    tokio::time::timeout(DEADLINE, f)
        .await
        .expect("fake-core I/O deadline exceeded")
}

impl FakeCore {
    /// Writes raw bytes in one call.
    pub async fn send(&mut self, bytes: &[u8]) {
        bounded(self.out.write_all(bytes)).await.unwrap();
    }

    /// Reads the hello request line and returns it parsed.
    pub async fn read_hello(&mut self) -> Value {
        let line = bounded(self.reader.read_line()).await.unwrap().unwrap();
        serde_json::from_slice(&line).unwrap()
    }

    /// Answers a hello already read with the given method/feature list.
    pub async fn answer_hello(&mut self, framed: bool, methods: &[&str], features: &[&str]) {
        self.send_line(&json!({
            "jsonrpc": "2.0", "id": 0,
            "result": {
                "generation": 1, "version": "fake-core 0.0", "framed": framed,
                "core": {"id": "core-fake-1", "name": "fake-core"},
                "methods": methods, "features": features,
            }
        }))
        .await;
    }

    /// `read_hello` + `answer_hello` in framed mode (Attach/Client
    /// conns). Advertises `automatic_input` + `resize_notifications`
    /// so negotiated-path tests see the features a real core offers.
    pub async fn serve_hello(&mut self) {
        let req = self.read_hello().await;
        assert_eq!(req["method"], "hello");
        self.answer_hello(
            req["params"]["framed"] == true,
            METHODS,
            &[
                hc_proto::hello::feature::AUTOMATIC_INPUT,
                hc_proto::hello::feature::RESIZE_NOTIFICATIONS,
            ],
        )
        .await;
    }

    /// One frame from the client.
    pub async fn read_frame(&mut self) -> Frame {
        bounded(self.reader.read_frame()).await.unwrap().unwrap()
    }

    /// Like `read_frame` but `None` once the client hung up.
    pub async fn try_read_frame(&mut self) -> Option<Frame> {
        bounded(self.reader.read_frame()).await.unwrap()
    }

    /// The next channel-0 request; stream frames are skipped.
    pub async fn read_request(&mut self) -> Value {
        loop {
            let f = self.read_frame().await;
            if f.channel == 0 && f.kind == Kind::Data {
                return serde_json::from_slice(&f.payload).unwrap();
            }
        }
    }

    /// The `terminal.attach` request + a reply; returns the request.
    pub async fn serve_attach(&mut self, result: Value) -> Value {
        self.serve_hello().await;
        let req = self.read_request().await;
        assert_eq!(req["method"], "terminal.attach");
        self.reply(&req["id"], result).await;
        req
    }

    /// Replies `result` to request `id` as a channel-0 DATA frame.
    pub async fn reply(&mut self, id: &Value, result: Value) {
        let payload =
            serde_json::to_vec(&json!({"jsonrpc": "2.0", "id": id, "result": result})).unwrap();
        self.send(&Frame::data(0, payload).encode()).await;
    }

    /// A stream frame sent raw.
    pub async fn send_stream(&mut self, channel: u32, kind: Kind, payload: &[u8]) {
        self.send(&Frame::new(channel, kind, payload.to_vec()).encode())
            .await;
    }

    /// Reads the next stream frame on `channel`, skipping channel-0.
    pub async fn read_stream(&mut self, channel: u32) -> Frame {
        loop {
            let f = self.read_frame().await;
            if f.channel == channel {
                return f;
            }
        }
    }

    /// Reads a plain-mode line (subscription wire).
    pub async fn read_message(&mut self) -> Value {
        let line = bounded(self.reader.read_line()).await.unwrap().unwrap();
        serde_json::from_slice(&line).unwrap()
    }

    /// One plain-mode line.
    pub async fn send_line(&mut self, message: &Value) {
        let mut line = serde_json::to_vec(message).unwrap();
        line.push(b'\n');
        self.send(&line).await;
    }

    /// A plain-mode reply.
    pub async fn reply_line(&mut self, id: &Value, result: Value) {
        self.send_line(&json!({"jsonrpc": "2.0", "id": id, "result": result}))
            .await;
    }

    /// One server notification as a plain-mode line.
    pub async fn notify(&mut self, method: &str, params: Value) {
        self.send_line(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
            .await;
    }
}

/// A `terminal.attach` result: stream channel, record, capability flags.
pub fn attach_result(channel: u32, terminal: Value, resize_notifications: bool) -> Value {
    json!({
        "channel": channel,
        "terminal": terminal,
        "resize_notifications": resize_notifications,
        "automatic_input": true,
    })
}

/// The smallest terminal record the schema allows; tests add fields.
pub fn minimal_terminal() -> Value {
    json!({
        "address": "t7",
        "labels": {},
        "argv": ["sh"],
        "cwd": "/home/user",
        "state": "live",
        "created_ms": 1700000000000u64,
        "screen": "exact",
        "cols": 80,
        "rows": 24,
    })
}

/// Openers over fresh duplex pairs: every connect pushes the server half
/// to `accepted`, like a real transport per attempt.
pub fn openers(accepted: mpsc::Sender<FakeCore>) -> (OpenRpc, OpenSub) {
    let tx = accepted.clone();
    let rpc: OpenRpc = std::sync::Arc::new(move || {
        let tx = tx.clone();
        Box::pin(async move {
            let (core, read, write) = pair(64 * 1024);
            if tx.send(core).await.is_err() {
                return Err(hc_proto::client::Error::Timeout("fake accept"));
            }
            Client::open(read, write, None).await
        })
    });
    let sub: OpenSub = std::sync::Arc::new(move |params: SubscribeParams| {
        let tx = accepted.clone();
        Box::pin(async move {
            let (core, read, write) = pair(64 * 1024);
            if tx.send(core).await.is_err() {
                return SubAttempt::Failed(SubscribeError::Open(hc_proto::client::Error::Timeout(
                    "fake accept",
                )));
            }
            match Subscription::open(read, write, None, &params).await {
                Ok(s) => SubAttempt::Subscribed(Box::new(s)),
                Err(SubscribeError::Call(CallError::Unsupported { reason, .. })) => {
                    SubAttempt::Unsupported(reason)
                }
                Err(e) => SubAttempt::Failed(e),
            }
        })
    });
    (rpc, sub)
}

/// Serves every accepted conn: `serve` gets each one after its hello was
/// read, classified `true` for framed (RPC) / `false` for plain (sub).
pub fn spawn_fake(
    mut accepted: mpsc::Receiver<FakeCore>,
    mut serve: impl FnMut(FakeCore, bool) -> BoxFuture<()> + Send + Sync + 'static,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(mut core) = accepted.recv().await {
            let hello = core.read_hello().await;
            let work = serve(core, hello["params"]["framed"] == true);
            tokio::spawn(work);
        }
    })
}

/// The plain-conn script's head: plain hello + the subscribe request
/// answered with `snapshot`. The caller drives notifications after.
pub async fn serve_subscribe(core: &mut FakeCore, snapshot: Value) -> Value {
    core.answer_hello(false, METHODS, &[]).await;
    let req = core.read_message().await;
    assert_eq!(req["method"], "events.subscribe");
    core.reply_line(&req["id"], snapshot).await;
    req
}

/// An rpc conn: hello, `doc.get`/`machine.list`/`terminal.list` answered
/// from fixed values, everything else replies `null`.
pub async fn serve_rpc(mut core: FakeCore) {
    core.answer_hello(true, METHODS, &[]).await;
    while let Some(f) = core.try_read_frame().await {
        if f.channel != 0 || f.kind != Kind::Data {
            continue;
        }
        let req: Value = serde_json::from_slice(&f.payload).unwrap();
        let result = match req["method"].as_str().unwrap_or("") {
            "rpc.discover" => {
                json!({"openrpc": "1.2.7", "info": {"title": "f", "version": "0"}, "methods": []})
            }
            "machine.list" => json!({"machines": []}),
            "doc.get" => json!({"value": null, "version": 1}),
            "terminal.list" => json!({"terminals": []}),
            _ => Value::Null,
        };
        core.reply(&req["id"], result).await;
    }
}

/// Waits under a deadline for a watcher view satisfying `pred`.
pub async fn await_view(
    watcher: &hc_proto::watch::Watcher,
    pred: impl Fn(&hc_proto::watch::View) -> bool,
) -> hc_proto::watch::View {
    tokio::time::timeout(DEADLINE, async {
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

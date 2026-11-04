//! A scripted fake core for client tests: speaks `hello` in plain mode, then
//! channel-0 DATA frames. `mod common;` in a test target pulls this in; not
//! every target uses every helper.
#![allow(dead_code)]

use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use hc_proto::transport::{ReadHalf, WriteHalf};
use serde_json::{Value, json};
use tokio::io::{
    AsyncRead, AsyncWrite, AsyncWriteExt, DuplexStream, ReadHalf as IoReadHalf,
    WriteHalf as IoWriteHalf,
};

/// Every method this client's typed surface calls.
pub const ALL_METHODS: &[&str] = &[
    "hello",
    "rpc.discover",
    "terminal.list",
    "terminal.get",
    "terminal.run",
    "terminal.read",
    "terminal.kill",
    "machine.list",
    "doc.get",
];

/// A `hello` result for generation 1, framed, with the given method list.
pub fn hello_result(methods: &[&str]) -> Value {
    json!({
        "generation": 1,
        "version": "fake-core 0.0",
        "framed": true,
        "core": {"id": "core-fake-1", "name": "fake-core"},
        "methods": methods,
        "features": [],
    })
}

/// The fake end of a client wire: a buffered reader plus raw write control
/// (one write can carry the hello reply and the first frame, exercising
/// read-ahead). Generic over the stream halves so duplex pairs and unix
/// socket halves both fit. Tests script it as a `tokio::spawn`ed task.
pub struct FakeCore<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> {
    reader: Reader<R>,
    out: W,
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> FakeCore<R, W> {
    /// A fake core over any read/write halves (e.g. a unix socket's).
    pub fn new(read: R, write: W) -> Self {
        FakeCore {
            reader: Reader::new(read),
            out: write,
        }
    }
}

/// A duplex-backed fake core.
pub type DuplexCore = FakeCore<IoReadHalf<DuplexStream>, IoWriteHalf<DuplexStream>>;

/// Builds a connected `(fake core, client read, client write)` triple.
pub fn pair(capacity: usize) -> (DuplexCore, ReadHalf, WriteHalf) {
    let (core_side, client_side) = tokio::io::duplex(capacity);
    let (cr, cw) = tokio::io::split(core_side);
    let (read, write) = tokio::io::split(client_side);
    (FakeCore::new(cr, cw), Box::pin(read), Box::pin(write))
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> FakeCore<R, W> {
    /// Writes raw bytes in one call — the way a real packet arrives.
    pub async fn send(&mut self, bytes: &[u8]) {
        self.out.write_all(bytes).await.unwrap();
    }

    /// Reads the hello request line and returns it parsed.
    pub async fn read_hello(&mut self) -> Value {
        let line = self.reader.read_line().await.unwrap().unwrap();
        serde_json::from_slice(&line).unwrap()
    }

    /// `read_hello` + `answer_hello`: the common handshake.
    pub async fn serve_hello(&mut self, methods: &[&str]) {
        let request = self.read_hello().await;
        assert_eq!(request["method"], "hello");
        assert_eq!(request["params"]["framed"], true);
        self.answer_hello(hello_result(methods)).await;
    }

    /// Writes a plain-mode hello result line.
    pub async fn answer_hello(&mut self, result: Value) {
        self.send_line(&json!({"jsonrpc": "2.0", "id": 0, "result": result}))
            .await;
    }

    /// Writes a plain-mode hello error line.
    pub async fn answer_hello_error(&mut self, kind: &str, hint: &str) {
        self.send_line(&json!({
            "jsonrpc": "2.0", "id": 0,
            "error": {"code": -32000, "message": kind, "data": {"kind": kind, "hint": hint}},
        }))
        .await;
    }

    /// One JSON message as a plain-mode line.
    pub async fn send_line(&mut self, message: &Value) {
        let mut line = serde_json::to_vec(message).unwrap();
        line.push(b'\n');
        self.send(&line).await;
    }

    /// Reads one channel-0 DATA request; other frames are skipped.
    pub async fn read_request(&mut self) -> Value {
        loop {
            let frame = self.reader.read_frame().await.unwrap().unwrap();
            if frame.channel == 0 && frame.kind == Kind::Data {
                return serde_json::from_slice(&frame.payload).unwrap();
            }
        }
    }

    /// Replies `result` to the request `id` (a parsed `req["id"]` Value).
    pub async fn reply(&mut self, id: &Value, result: Value) {
        self.reply_value(&json!({"jsonrpc": "2.0", "id": id, "result": result}))
            .await;
    }

    /// Replies a structured `kind`/`hint` error to `id`.
    pub async fn reply_error(&mut self, id: &Value, kind: &str, hint: &str) {
        self.reply_value(&json!({
            "jsonrpc": "2.0", "id": id,
            "error": {"code": -32000, "message": kind, "data": {"kind": kind, "hint": hint}},
        }))
        .await;
    }

    /// One reply as a channel-0 DATA frame (no newline, as core sends).
    async fn reply_value(&mut self, reply: &Value) {
        let payload = serde_json::to_vec(reply).unwrap();
        self.send(&Frame::data(0, payload).encode()).await;
    }

    /// Asserts the client's write side ended the stream cleanly.
    pub async fn expect_eof(&mut self) {
        assert_eq!(self.reader.read_frame().await.unwrap(), None);
    }

    /// Reads until the client's write side ends; returns how many complete
    /// frames arrived first. A trailing truncated frame (a write that died
    /// mid-flight) also ends the loop.
    pub async fn drain_until_eof(&mut self) -> usize {
        let mut n = 0;
        while let Ok(Some(_)) = self.reader.read_frame().await {
            n += 1;
        }
        n
    }

    /// Splits the fake so a test can kill one direction only: dropping the
    /// returned reader breaks the client's writer; dropping `out` breaks
    /// the client's reader.
    pub fn into_parts(self) -> (Reader<R>, W) {
        (self.reader, self.out)
    }

    /// Answers the `rpc.discover` call the client makes when hello offers
    /// it, declaring `methods`. For scripts where the discovery call is
    /// expected at this exact point in the request order.
    pub async fn expect_discover(&mut self, methods: &[&str]) {
        let req = self.read_request().await;
        assert_eq!(req["method"], "rpc.discover");
        self.reply(&req["id"], discover_doc(methods)).await;
    }
}

/// A valid OpenRPC-shaped discovery document declaring `methods`.
pub fn discover_doc(methods: &[&str]) -> Value {
    let list: Vec<Value> = methods.iter().map(|name| json!({"name": name})).collect();
    json!({"openrpc": "1.2.7", "info": {"title": "fake", "version": "0"}, "methods": list})
}

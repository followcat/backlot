//! The recording driver and the fake core the seam tests share: a
//! driver that forwards to a [`LocalDriver`] and logs each call, and a
//! core that answers every terminal method so an action runs.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use hc_proto::client::{CallError, Client};
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use hc_proto::terminal::{ReadResult, RunParams, RunResult, Terminal};
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use super::Log;
use crate::driver::{Fut, LocalDriver, MachineDriver, SendHandle};

/// A driver that forwards every call to a [`LocalDriver`] and records
/// its name — the call the stage would still make today, spelled as the
/// seam's method.
pub(super) struct RecordingDriver {
    inner: LocalDriver,
    log: Log,
}

impl RecordingDriver {
    pub(super) fn new(git: crate::driver::SharedGit) -> (Box<RecordingDriver>, super::Log) {
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        (
            Box::new(RecordingDriver {
                inner: LocalDriver::with_git(git),
                log: Arc::clone(&log),
            }),
            log,
        )
    }

    fn note(&self, call: String) {
        self.log.lock().unwrap().push(call);
    }
}

impl MachineDriver for RecordingDriver {
    fn run<'a>(
        &self,
        core: &'a Client,
        params: RunParams,
    ) -> Fut<'a, Result<RunResult, CallError>> {
        self.note(format!("run {}", params.name.clone().unwrap_or_default()));
        self.inner.run(core, params)
    }

    fn get<'a>(&self, core: &'a Client, terminal: &str) -> Fut<'a, Result<Terminal, CallError>> {
        self.note(format!("get {terminal}"));
        self.inner.get(core, terminal)
    }

    fn kill<'a>(&self, core: &'a Client, terminal: &str) -> Fut<'a, Result<Terminal, CallError>> {
        self.note(format!("kill {terminal}"));
        self.inner.kill(core, terminal)
    }

    fn rm<'a>(&self, core: &'a Client, terminal: &str) -> Fut<'a, Result<Value, CallError>> {
        self.note(format!("rm {terminal}"));
        self.inner.rm(core, terminal)
    }

    fn read<'a>(&self, core: &'a Client, terminal: &str) -> Fut<'a, Result<ReadResult, CallError>> {
        self.note(format!("read {terminal}"));
        self.inner.read(core, terminal)
    }

    fn send<'a>(
        &self,
        core: &'a Client,
        terminal: &str,
        text: &str,
    ) -> Fut<'a, Result<Value, CallError>> {
        self.note(format!("send {terminal} {text}"));
        self.inner.send(core, terminal, text)
    }

    fn send_handle(&self, core: &Client) -> SendHandle {
        let inner = self.inner.send_handle(core);
        let log = Arc::clone(&self.log);
        Arc::new(
            move |terminal: String, text: String, deadline: std::time::Duration| {
                log.lock().unwrap().push(format!("send {terminal} {text}"));
                inner(terminal, text, deadline)
            },
        )
    }

    fn keys<'a>(
        &self,
        core: &'a Client,
        terminal: &str,
        keys: &[&str],
    ) -> Fut<'a, Result<Value, CallError>> {
        self.note(format!("keys {terminal} {}", keys.join(",")));
        self.inner.keys(core, terminal, keys)
    }

    fn label<'a>(
        &self,
        core: &'a Client,
        terminal: &str,
        labels: &BTreeMap<String, String>,
    ) -> Fut<'a, Result<Value, CallError>> {
        self.note(format!("label {terminal}"));
        self.inner.label(core, terminal, labels)
    }

    fn keep<'a>(
        &self,
        core: &'a Client,
        terminal: &str,
        keep: bool,
    ) -> Fut<'a, Result<Value, CallError>> {
        self.note(format!("keep {terminal} {keep}"));
        self.inner.keep(core, terminal, keep)
    }

    fn supports(&self, core: &Client, method: &str) -> bool {
        self.inner.supports(core, method)
    }

    fn git(&self) -> std::cell::RefMut<'_, Box<crate::workareas::GitRun<'static>>> {
        self.inner.git()
    }

    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        self.note(format!("create_dir_all {}", path.display()));
        self.inner.create_dir_all(path)
    }

    fn write_file(&self, path: &Path, body: &[u8]) -> std::io::Result<()> {
        self.note(format!("write_file {}", path.display()));
        self.inner.write_file(path, body)
    }

    fn write_executable(&self, path: &Path, body: &str) -> std::io::Result<()> {
        self.note(format!("write_executable {}", path.display()));
        self.inner.write_executable(path, body)
    }

    fn read_file(&self, path: &Path) -> std::io::Result<String> {
        self.note(format!("read_file {}", path.display()));
        self.inner.read_file(path)
    }
}

// ---------------------------------------------------------------------
// The fake core: `hello` offering every method, then one answer per
// call so the stage acts rather than refusing.
// ---------------------------------------------------------------------

const METHODS: &[&str] = &[
    "terminal.run",
    "terminal.kill",
    "terminal.rm",
    "terminal.read",
    "terminal.send",
    "terminal.keys",
    "terminal.label",
    "terminal.list",
];

fn hello_reply() -> Vec<u8> {
    let methods: Vec<Value> = METHODS.iter().map(|m| json!(m)).collect();
    serde_json::to_string(&json!({
        "jsonrpc": "2.0", "id": 0,
        "result": {
            "generation": 1, "version": "fake-core 0.0",
            "framed": true,
            "core": {"id": "core-fake-1", "name": "fake-core"},
            "methods": methods,
            "features": [],
        }
    }))
    .unwrap()
    .into_bytes()
}

fn terminal(address: &str, name: &str) -> Value {
    json!({
        "address": address,
        "name": name,
        "labels": {},
        "argv": ["fake"],
        "cwd": "/home/user/acme-shop",
        "state": "live",
        "created_ms": 0,
        "screen": "",
        "cols": 80,
        "rows": 24,
    })
}

async fn serve(stream: DuplexStream) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = Reader::new(read);
    let hello = reader.read_line().await.unwrap().unwrap();
    let request: Value = serde_json::from_slice(&hello).unwrap();
    assert_eq!(request["method"], "hello");
    write.write_all(&hello_reply()).await.unwrap();
    write.write_all(b"\n").await.unwrap();
    while let Ok(Some(frame)) = reader.read_frame().await {
        if frame.channel != 0 || frame.kind != Kind::Data {
            continue;
        }
        let request: Value = serde_json::from_slice(&frame.payload).unwrap();
        let name = request["params"]["name"]
            .as_str()
            .unwrap_or("bl-acme-producer");
        let result = match request["method"].as_str().unwrap_or("") {
            "terminal.run" => terminal("t7", name),
            "terminal.read" => json!({"text": "", "complete": true, "how": "screen"}),
            "terminal.send" | "terminal.keys" | "terminal.label" | "terminal.rm" => json!({}),
            "terminal.kill" => terminal("t7", name),
            other => panic!("the stage called an unexpected method: {other}"),
        };
        let reply = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": request["id"], "result": result,
        }))
        .unwrap();
        write
            .write_all(&Frame::data(0, reply).encode())
            .await
            .unwrap();
    }
}

pub(super) async fn client_on_fake_core() -> Client {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    tokio::spawn(serve(core_side));
    let (read, write) = tokio::io::split(client_side);
    Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap()
}

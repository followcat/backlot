//! Stateful in-memory fake herdr-core over a Unix socket for B1a doc tests.
//!
//! Implements `hello`, `doc.get`, `doc.set` (with uint64 CAS and optional
//! concurrent-append conflict injection), `terminal.list`, and `events.subscribe`
//! with live `doc` change notifications.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::endpoint::{Endpoint, Local};
use hc_proto::frame::{Frame, Kind};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;

#[derive(Default)]
pub(crate) struct FakeDocState {
    docs: BTreeMap<String, (u64, Value)>,
    writes: BTreeMap<String, usize>,
    next_seq: u64,
    inject_on_next_req_set: Option<Value>,
}

#[derive(Clone)]
pub(crate) struct FakeCore {
    socket: PathBuf,
    state: Arc<Mutex<FakeDocState>>,
    tx: broadcast::Sender<(u64, String, u64)>,
}

impl FakeCore {
    pub(crate) async fn spawn(socket: PathBuf) -> Self {
        let listener = UnixListener::bind(&socket).expect("bind fake core socket");
        let state = Arc::new(Mutex::new(FakeDocState::default()));
        let (tx, _) = broadcast::channel(64);
        let core = FakeCore {
            socket,
            state: state.clone(),
            tx: tx.clone(),
        };
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let state = state.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    serve_conn(stream, state, tx).await;
                });
            }
        });
        core
    }

    pub(crate) fn endpoint(&self) -> Endpoint {
        Endpoint::Local(Local {
            session: "fake-session".into(),
            socket: self.socket.clone(),
            holder_socket: self.socket.with_extension("holder"),
            state: self.socket.parent().unwrap().to_path_buf(),
            config: self.socket.with_extension("toml"),
        })
    }

    pub(crate) async fn client(&self) -> Client {
        Client::connect(&self.endpoint(), None)
            .await
            .expect("connect fake core client")
    }

    pub(crate) fn doc(&self, name: &str) -> Option<(u64, Value)> {
        self.state.lock().unwrap().docs.get(name).cloned()
    }

    pub(crate) fn doc_writes(&self, name: &str) -> usize {
        self.state
            .lock()
            .unwrap()
            .writes
            .get(name)
            .copied()
            .unwrap_or(0)
    }

    pub(crate) fn set_raw_doc(&self, name: &str, value: Value) -> u64 {
        let ver = {
            let mut g = self.state.lock().unwrap();
            let next_v = g.docs.get(name).map(|(v, _)| *v + 1).unwrap_or(1);
            g.docs.insert(name.to_string(), (next_v, value));
            *g.writes.entry(name.to_string()).or_insert(0) += 1;
            g.next_seq += 1;
            let seq = g.next_seq;
            (next_v, seq)
        };
        let _ = self.tx.send((ver.1, name.to_string(), ver.0));
        ver.0
    }

    /// Arm a one-shot concurrent append to `plugin.backlot.req`: when the next
    /// `doc.set("plugin.backlot.req", ...)` arrives, inject `entry` cleanly into
    /// the stored queue first, advance its version, and return `conflict`.
    pub(crate) fn inject_concurrent_req_on_next_set(&self, entry: Value) {
        self.state.lock().unwrap().inject_on_next_req_set = Some(entry);
    }
}

async fn serve_conn(
    stream: UnixStream,
    state: Arc<Mutex<FakeDocState>>,
    tx: broadcast::Sender<(u64, String, u64)>,
) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = Reader::new(read);
    let Ok(Some(hello)) = reader.read_line().await else {
        return;
    };
    let req: Value = serde_json::from_slice(&hello).unwrap();
    let framed = req
        .get("params")
        .and_then(|p| p.get("framed"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let reply = serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": req["id"],
        "result": {
            "generation": 1,
            "version": "fake-core 0.0",
            "framed": framed,
            "core": {"id": "core-fake-1", "name": "fake-core"},
            "methods": [
                "terminal.list",
                "events.subscribe",
                "doc.get",
                "doc.set",
            ],
            "features": [],
        }
    }))
    .unwrap();
    if write.write_all(&reply).await.is_err() || write.write_all(b"\n").await.is_err() {
        return;
    }

    let mut sub_rx: Option<broadcast::Receiver<(u64, String, u64)>> = None;
    loop {
        if let Some(rx) = sub_rx.as_mut() {
            tokio::select! {
                ev = rx.recv() => {
                    if let Ok((seq, name, version)) = ev {
                        let note = serde_json::to_vec(&json!({
                            "jsonrpc": "2.0",
                            "method": "event",
                            "params": {
                                "seq": seq,
                                "type": "doc",
                                "data": { "name": name, "version": version }
                            }
                        }))
                        .unwrap();
                        if !send_payload(&mut write, framed, note).await {
                            break;
                        }
                    }
                }
                incoming = read_payload(&mut reader, framed) => {
                    let Some(payload) = incoming else { break };
                    if !handle_payload(payload, framed, &state, &tx, &mut write, &mut sub_rx).await {
                        break;
                    }
                }
            }
        } else {
            let Some(payload) = read_payload(&mut reader, framed).await else {
                break;
            };
            if !handle_payload(payload, framed, &state, &tx, &mut write, &mut sub_rx).await {
                break;
            }
        }
    }
}

async fn read_payload<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut Reader<R>,
    framed: bool,
) -> Option<Vec<u8>> {
    if framed {
        loop {
            let frame = reader.read_frame().await.ok()??;
            if frame.channel == 0 && frame.kind == Kind::Data {
                return Some(frame.payload);
            }
        }
    } else {
        reader.read_line().await.ok()?
    }
}

async fn send_payload<W: AsyncWriteExt + Unpin>(
    write: &mut W,
    framed: bool,
    payload: Vec<u8>,
) -> bool {
    if framed {
        write
            .write_all(&Frame::data(0, payload).encode())
            .await
            .is_ok()
    } else {
        let mut line = payload;
        line.push(b'\n');
        write.write_all(&line).await.is_ok()
    }
}

async fn handle_payload<W: AsyncWriteExt + Unpin>(
    payload: Vec<u8>,
    framed: bool,
    state: &Arc<Mutex<FakeDocState>>,
    tx: &broadcast::Sender<(u64, String, u64)>,
    write: &mut W,
    sub_rx: &mut Option<broadcast::Receiver<(u64, String, u64)>>,
) -> bool {
    let req: Value = serde_json::from_slice(&payload).unwrap();
    let method = req["method"].as_str().unwrap_or("");
    let params = &req["params"];
    let mut notify: Option<(u64, String, u64)> = None;

    let response = match method {
        "terminal.list" => json!({
            "jsonrpc": "2.0",
            "id": req["id"],
            "result": { "terminals": [] }
        }),
        "events.subscribe" => {
            *sub_rx = Some(tx.subscribe());
            let seq = state.lock().unwrap().next_seq;
            json!({
                "jsonrpc": "2.0",
                "id": req["id"],
                "result": { "seq": seq, "terminals": [] }
            })
        }
        "doc.get" => {
            let name = params["name"].as_str().unwrap_or("");
            let g = state.lock().unwrap();
            let (version, value) = g.docs.get(name).cloned().unwrap_or((0, Value::Null));
            json!({
                "jsonrpc": "2.0",
                "id": req["id"],
                "result": { "value": value, "version": version }
            })
        }
        "doc.set" => {
            let name = params["name"].as_str().unwrap_or("").to_string();
            let want_ver = params["version"].as_u64().unwrap_or(0);
            let value = params["value"].clone();
            let mut g = state.lock().unwrap();
            let cur_ver = g.docs.get(&name).map(|(v, _)| *v).unwrap_or(0);

            if name == backlot_model::req_wire::REQ_DOC_NAME
                && let Some(extra) = g.inject_on_next_req_set.take()
            {
                let mut existing = g
                    .docs
                    .get(&name)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_else(|| json!({"schema": "backlot-req/1", "queue": []}));
                if let Some(arr) = existing.get_mut("queue").and_then(Value::as_array_mut) {
                    arr.push(extra);
                }
                let bumped = cur_ver + 1;
                g.docs.insert(name.clone(), (bumped, existing));
                json!({
                    "jsonrpc": "2.0",
                    "id": req["id"],
                    "error": {
                        "code": -32000,
                        "message": "conflict",
                        "data": { "kind": "conflict" }
                    }
                })
            } else if cur_ver != want_ver {
                json!({
                    "jsonrpc": "2.0",
                    "id": req["id"],
                    "error": {
                        "code": -32000,
                        "message": "conflict",
                        "data": { "kind": "conflict" }
                    }
                })
            } else {
                let next_ver = cur_ver + 1;
                g.docs.insert(name.clone(), (next_ver, value));
                *g.writes.entry(name.clone()).or_insert(0) += 1;
                g.next_seq += 1;
                notify = Some((g.next_seq, name, next_ver));
                json!({
                    "jsonrpc": "2.0",
                    "id": req["id"],
                    "result": { "version": next_ver }
                })
            }
        }
        _ => json!({
            "jsonrpc": "2.0",
            "id": req["id"],
            "error": { "code": -32601, "message": "method not found" }
        }),
    };

    let bytes = serde_json::to_vec(&response).unwrap();
    if !send_payload(write, framed, bytes).await {
        return false;
    }
    if let Some(ev) = notify {
        let _ = tx.send(ev);
    }
    true
}

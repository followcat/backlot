//! Self-contained fake `hc` core socket server for `server_*` tests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;

use hc_proto::codec::{Reader, Writer};
use hc_proto::doc::{DocGetParams, DocGetResult, DocSetParams, DocSetResult};
use hc_proto::events::SubscribeResult;
use hc_proto::frame::{CONTROL_CHANNEL, Frame, Kind};
use hc_proto::hello::{CoreInfo, HelloParams, HelloResult};
use hc_proto::jsonrpc::{
    ErrResponse, Id, OkResponse, Request, Response, RpcError, Version, error_kind,
};
use hc_proto::method;
use hc_proto::terminal::{ListResult, RunParams, Terminal, TerminalParams};
use serde_json::Value;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;

#[derive(Default)]
struct State {
    core_id: String,
    terminals: Vec<Terminal>,
    next_id: u32,
    docs: BTreeMap<String, (u64, Value)>,
    kills: Vec<String>,
    rms: Vec<String>,
    runs: Vec<RunParams>,
}

/// Fake `hc` core running on a dedicated OS thread with a Unix socket.
pub(crate) struct FakeHcCore {
    pub(crate) socket: PathBuf,
    state: Arc<Mutex<State>>,
    shutdown: broadcast::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl FakeHcCore {
    pub(crate) fn spawn(socket: PathBuf, core_id: &str) -> Self {
        let _ = std::fs::remove_file(&socket);
        if let Some(parent) = socket.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let state = Arc::new(Mutex::new(State {
            core_id: core_id.to_string(),
            next_id: 1,
            ..State::default()
        }));
        let (shutdown, _) = broadcast::channel(4);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let sock_clone = socket.clone();
        let state_clone = state.clone();
        let sd_clone = shutdown.clone();
        let handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("fake core runtime");
            rt.block_on(async move {
                let listener = UnixListener::bind(&sock_clone).expect("bind fake hc socket");
                let _ = ready_tx.send(());
                let mut sd_rx = sd_clone.subscribe();
                loop {
                    tokio::select! {
                        _ = sd_rx.recv() => break,
                        res = listener.accept() => {
                            let Ok((stream, _)) = res else { break };
                            tokio::spawn(serve_conn(stream, state_clone.clone(), sd_clone.subscribe()));
                        }
                    }
                }
            });
        });
        ready_rx.recv().expect("fake core bound");
        Self {
            socket,
            state,
            shutdown,
            thread: Some(handle),
        }
    }

    pub(crate) fn add_terminal(
        &self,
        name: Option<&str>,
        labels: &[(&str, &str)],
        argv: &[&str],
        state: &str,
    ) -> Terminal {
        let mut st = self.state.lock().unwrap();
        let addr = format!("t{}", st.next_id);
        st.next_id += 1;
        let label_map = labels
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        let argv_vec = argv.iter().map(|s| (*s).to_string()).collect();
        let term = mk_terminal(&addr, name, label_map, argv_vec, state);
        st.terminals.push(term.clone());
        term
    }

    pub(crate) fn terminals(&self) -> Vec<Terminal> {
        self.state.lock().unwrap().terminals.clone()
    }

    pub(crate) fn killed(&self) -> Vec<String> {
        self.state.lock().unwrap().kills.clone()
    }

    pub(crate) fn rms(&self) -> Vec<String> {
        self.state.lock().unwrap().rms.clone()
    }

    pub(crate) fn ran(&self) -> Vec<RunParams> {
        self.state.lock().unwrap().runs.clone()
    }
}

impl Drop for FakeHcCore {
    fn drop(&mut self) {
        let _ = self.shutdown.send(());
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

fn mk_terminal(
    addr: &str,
    name: Option<&str>,
    labels: BTreeMap<String, String>,
    argv: Vec<String>,
    state: &str,
) -> Terminal {
    let mut v = serde_json::json!({
        "address": addr,
        "name": name,
        "labels": labels,
        "argv": argv,
        "cwd": "/tmp",
        "state": state,
        "pid": 4242,
        "created_ms": 1_700_000_000_000u64,
        "screen": "exact",
        "cols": 80,
        "rows": 24,
    });
    if state == "finished" {
        v["reason"] = serde_json::json!("killed");
    }
    serde_json::from_value(v).expect("terminal json")
}

async fn serve_conn(
    stream: UnixStream,
    state: Arc<Mutex<State>>,
    mut shutdown: broadcast::Receiver<()>,
) {
    let (r, w) = stream.into_split();
    let mut reader = Reader::new(r);
    let mut writer = Writer::new(w);

    let Ok(Some(first)) = reader.read_line().await else {
        return;
    };
    let Ok(req) = serde_json::from_slice::<Request>(&first) else {
        return;
    };
    if req.method != method::HELLO {
        return;
    }
    let params: HelloParams = req
        .params
        .clone()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let framed = params.framed;
    let core_id = state.lock().unwrap().core_id.clone();
    let hello = HelloResult {
        generation: hc_proto::PROTOCOL_GENERATION,
        version: "0.1.25".to_string(),
        core: CoreInfo {
            id: core_id,
            name: "host-a".to_string(),
            hostname: Some("host-a".to_string()),
        },
        framed,
        methods: [
            method::HELLO,
            method::TERMINAL_LIST,
            method::TERMINAL_GET,
            method::TERMINAL_RUN,
            method::TERMINAL_KILL,
            method::TERMINAL_RM,
            method::DOC_GET,
            method::DOC_SET,
            method::EVENTS_SUBSCRIBE,
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        features: Vec::new(),
    };
    let resp = ok_response(req.id, serde_json::to_value(hello).unwrap());
    if writer
        .write_line(&serde_json::to_vec(&resp).unwrap())
        .await
        .is_err()
    {
        return;
    }

    if !framed {
        loop {
            tokio::select! {
                _ = shutdown.recv() => break,
                line = reader.read_line() => {
                    let Ok(Some(bytes)) = line else { break };
                    let Ok(req) = serde_json::from_slice::<Request>(&bytes) else { break };
                    let resp = handle_request(&req, &state);
                    if writer.write_line(&serde_json::to_vec(&resp).unwrap()).await.is_err() {
                        break;
                    }
                    if req.method == method::EVENTS_SUBSCRIBE {
                        let _ = shutdown.recv().await;
                        break;
                    }
                }
            }
        }
        return;
    }

    loop {
        tokio::select! {
            _ = shutdown.recv() => break,
            frame = reader.read_frame() => {
                let Ok(Some(f)) = frame else { break };
                if f.channel != CONTROL_CHANNEL || f.kind != Kind::Data {
                    continue;
                }
                let Ok(req) = serde_json::from_slice::<Request>(&f.payload) else { continue };
                let resp = handle_request(&req, &state);
                let out = Frame::data(CONTROL_CHANNEL, serde_json::to_vec(&resp).unwrap());
                if writer.write_frame(&out).await.is_err() {
                    break;
                }
            }
        }
    }
}

fn handle_request(req: &Request, state: &Arc<Mutex<State>>) -> Response {
    match dispatch_request(req, state) {
        Ok(result) => Response::Ok(OkResponse {
            jsonrpc: Version,
            id: req.id.clone(),
            result,
        }),
        Err(error) => Response::Err(ErrResponse {
            jsonrpc: Version,
            id: Some(req.id.clone()),
            error,
        }),
    }
}

fn dispatch_request(req: &Request, state: &Arc<Mutex<State>>) -> Result<Value, RpcError> {
    let mut st = state.lock().unwrap();
    let val = match req.method.as_str() {
        method::TERMINAL_LIST => serde_json::to_value(ListResult {
            terminals: st.terminals.clone(),
            hidden_finished: 0,
            machines: Vec::new(),
        }),
        method::TERMINAL_GET => {
            let p: TerminalParams = parse_params(req)?;
            let t = find_term(&mut st.terminals, &p.terminal)?;
            serde_json::to_value(t)
        }
        method::TERMINAL_RUN => {
            let p: RunParams = parse_params(req)?;
            if let Some(name) = &p.name
                && st
                    .terminals
                    .iter()
                    .any(|t| t.state != "finished" && t.name.as_deref() == Some(name))
            {
                return Err(RpcError::new(
                    -32000,
                    error_kind::CONFLICT,
                    "duplicate name",
                    "",
                ));
            }
            let addr = format!("t{}", st.next_id);
            st.next_id += 1;
            let term = mk_terminal(
                &addr,
                p.name.as_deref(),
                p.labels.clone(),
                p.argv.clone(),
                "live",
            );
            st.runs.push(p);
            st.terminals.push(term.clone());
            serde_json::to_value(term)
        }
        method::TERMINAL_KILL => {
            let p: TerminalParams = parse_params(req)?;
            st.kills.push(p.terminal.clone());
            let t = find_term(&mut st.terminals, &p.terminal)?;
            t.state = "finished".to_string();
            t.reason = Some("killed".to_string());
            serde_json::to_value(t.clone())
        }
        method::TERMINAL_RM => {
            let p: TerminalParams = parse_params(req)?;
            st.rms.push(p.terminal.clone());
            st.terminals
                .retain(|t| t.address != p.terminal && t.name.as_deref() != Some(&p.terminal));
            Ok(Value::Object(Default::default()))
        }
        method::DOC_GET => {
            let p: DocGetParams = parse_params(req)?;
            let (version, value) = st.docs.get(&p.name).cloned().unwrap_or((0, Value::Null));
            serde_json::to_value(DocGetResult { value, version })
        }
        method::DOC_SET => {
            let p: DocSetParams = parse_params(req)?;
            let cur = st.docs.get(&p.name).map(|(v, _)| *v).unwrap_or(0);
            if p.version != cur {
                return Err(RpcError::new(
                    -32000,
                    error_kind::CONFLICT,
                    "version mismatch",
                    "",
                ));
            }
            let next = cur + 1;
            st.docs.insert(p.name, (next, p.value));
            serde_json::to_value(DocSetResult { version: next })
        }
        method::EVENTS_SUBSCRIBE => serde_json::to_value(SubscribeResult {
            seq: 0,
            terminals: st.terminals.clone(),
            channel: None,
        }),
        other => {
            return Err(RpcError::new(-32601, error_kind::UNKNOWN_METHOD, other, ""));
        }
    };
    Ok(val.expect("json serialize"))
}

fn find_term<'a>(terms: &'a mut [Terminal], id: &str) -> Result<&'a mut Terminal, RpcError> {
    terms
        .iter_mut()
        .find(|t| t.address == id || t.name.as_deref() == Some(id))
        .ok_or_else(|| RpcError::new(-32000, error_kind::NOT_FOUND, "missing", ""))
}

fn parse_params<T: serde::de::DeserializeOwned>(req: &Request) -> Result<T, RpcError> {
    serde_json::from_value(req.params.clone().unwrap_or(Value::Null))
        .map_err(|e| RpcError::new(-32602, error_kind::INVALID_PARAMS, e.to_string(), ""))
}

fn ok_response(id: Id, result: Value) -> Response {
    Response::Ok(OkResponse {
        jsonrpc: Version,
        id,
        result,
    })
}

pub(crate) fn base_env(root: &Path) -> BTreeMap<String, String> {
    let p = |sub: &str| root.join(sub).to_string_lossy().into_owned();
    BTreeMap::from([
        ("HOME".into(), p("home")),
        ("XDG_STATE_HOME".into(), p("xdg-state")),
        ("XDG_CONFIG_HOME".into(), p("xdg-config")),
        ("XDG_RUNTIME_DIR".into(), p("xdg-run")),
    ])
}

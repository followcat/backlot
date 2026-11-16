//! `--remote` steady state, proven at the spawn boundary: one scripted
//! `ssh` child serves the dedicated `events.subscribe` stream, one
//! serves the shared framed RPC wire the watcher's refreshes, the
//! layout store's actions and the seen worker's label writes all
//! multiplex onto, and one serves the shared `AttachConn` multiplexer —
//! three children, never one per consumer.
#![cfg(unix)]

use super::*;
use crate::operations::{Action, MachineOp};
use hc_proto::client::AttachConn;
use hc_proto::endpoint::{Endpoint, Remote};
use hc_proto::watch::{OpenRpc, WatchParams};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(15);

/// A scratch dir holding the scripted `ssh` and its spawn ledger —
/// every invocation appends one line before answering `hello`.
struct Fixture {
    dir: PathBuf,
    script: PathBuf,
    spawns: PathBuf,
}

impl Fixture {
    /// The script answers `hello` framed or plain by the request's
    /// `framed` flag — the three consumers are indistinguishable to it —
    /// answers the plain conn's `events.subscribe` with an empty
    /// snapshot, then sleeps holding every wire open: a live conn is a
    /// live conn, and pending calls never respawn.
    fn new() -> Fixture {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("hc-tui-remote-{}-{nanos:x}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let spawns = dir.join("spawns");
        let script = dir.join("fake-ssh");
        let hello = |framed: bool| {
            serde_json::to_string(&json!({
                "jsonrpc": "2.0", "id": 0,
                "result": {
                    "generation": 1, "version": "fake-core 0.0", "framed": framed,
                    "core": {"id": "core-fake-1", "name": "fake-core"},
                    "methods": [
                        "events.subscribe", "doc.get", "doc.set",
                        "machine.list", "machine.connect",
                        "terminal.list", "terminal.get", "terminal.label",
                        "terminal.attach",
                    ],
                    "features": [],
                }
            }))
            .unwrap()
        };
        let subscribe = serde_json::to_string(&json!({
            "jsonrpc": "2.0", "id": 1,
            "result": {"seq": 1, "terminals": []},
        }))
        .unwrap();
        let body = format!(
            "#!/bin/sh\n\
             echo spawn >> '{spawns}'\n\
             IFS= read -r hello\n\
             case \"$hello\" in\n\
             *'\"framed\":true'*) printf '%s\\n' '{framed}' ;;\n\
             *) printf '%s\\n' '{plain}'\n\
             IFS= read -r _subscribe\n\
             printf '%s\\n' '{subscribe}' ;;\n\
             esac\n\
             exec sleep 60\n",
            spawns = spawns.display(),
            framed = hello(true),
            plain = hello(false),
        );
        std::fs::write(&script, body).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        Fixture {
            dir,
            script,
            spawns,
        }
    }

    fn count(&self) -> usize {
        std::fs::read_to_string(&self.spawns).map_or(0, |s| s.lines().count())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn deadline_wait(done: impl Fn() -> bool) {
    tokio::time::timeout(DEADLINE, async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("deadline exceeded waiting for ssh spawns");
}

/// The remote wiring `cli::run` builds: the watcher's subscription and
/// refreshes over the scripted `ssh`, the shared attach multiplexer on
/// the same program, and `set_operations`' store + seen worker on the
/// shared `rpc` opener.
fn remote_app(endpoint: &Endpoint, ssh: &std::ffi::OsString, rpc: OpenRpc) -> App {
    let watcher = Watcher::connect_with_rpc(
        endpoint,
        Some(ssh.as_os_str()),
        rpc.clone(),
        WatchParams {
            traversal: Some(crate::project::agent_traversal),
            ..WatchParams::default()
        },
    );
    let attach_conn = AttachConn::new(endpoint.clone(), Some(ssh.clone()));
    let open: OpenAttach = Arc::new(move |params| {
        let conn = attach_conn.clone();
        Box::pin(async move { conn.attach(&params).await })
    });
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
    app
}

#[tokio::test]
async fn remote_steady_state_is_three_ssh_children() {
    let fixture = Fixture::new();
    let ssh = fixture.script.clone().into_os_string();
    let endpoint = Endpoint::Remote(Remote {
        target: "host-b".into(),
        session: "scratch".into(),
    });
    let rpc = hc_proto::client::shared_rpc(endpoint.clone(), Some(ssh.clone()));
    let mut app = remote_app(&endpoint, &ssh, rpc.clone());
    app.set_operations(rpc);

    // A focused live pane: the reconcile arms the seen write on the
    // shared wire and wants the active attach on the multiplexer.
    publish_agent(&mut app, "t1", 5);

    // The subscription, the shared RPC and the attach multiplexer.
    deadline_wait(|| fixture.count() == 3).await;

    // A store action and another turn's seen write ride the shared
    // conn — never a child of their own. Their calls park on the
    // sleeping script; only the spawn boundary is asserted.
    app.submit_action(Action::Machine(MachineOp::Connect {
        machine: "host-a".into(),
    }));
    publish_agent(&mut app, "t1", 6);
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        fixture.count(),
        3,
        "a consumer opened its own ssh child; expected one shared RPC wire"
    );
}

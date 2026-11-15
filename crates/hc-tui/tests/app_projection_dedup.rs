//! `Probe::projections` end-to-end: publishes that carry no
//! projection-relevant change never rebuild the projection, and the
//! startup publish storm projects at most once per endpoint.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use hc_proto::watch::{OpenRpc, WatchParams, Watcher};
use hc_tui::app::{App, Probe};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};
use tokio::sync::mpsc;

#[path = "common/loopcore.rs"]
mod loopcore;
use loopcore::fakecore;

/// `OpenRpc` over fresh duplex pairs; the driver serves every accepted
/// conn's hello plus `machine.list`/`doc.get` requests.
fn rpc_opens(accepted: mpsc::Sender<fakecore::DuplexCore>) -> OpenRpc {
    Arc::new(move || {
        let accepted = accepted.clone();
        Box::pin(async move {
            let (core, read, write) = fakecore::pair(64 * 1024);
            if accepted.send(core).await.is_err() {
                return Err(hc_proto::client::Error::Timeout("fake accept"));
            }
            hc_proto::client::Client::open(read, write, None).await
        })
    })
}

fn machine_json(name: &str) -> Value {
    json!({
        "name": name, "target": "alice@host", "id": format!("core-{name}"),
        "host": name, "version": "0.9", "state": "connected",
    })
}

/// Serves one rpc conn: `machine.list` from `machines`, `doc.get` as a
/// null read (each reply bumps `doc_gets`), anything else `null`.
async fn serve_rpc_conn(
    mut core: fakecore::DuplexCore,
    machines: Vec<Value>,
    doc_gets: Arc<AtomicU64>,
) {
    core.serve_hello(loopcore::METHODS).await;
    while let Some(request) = core.try_read_request().await {
        match request["method"].as_str().unwrap() {
            "machine.list" => {
                core.reply(&request["id"], json!({"machines": machines}))
                    .await;
            }
            "doc.get" => {
                core.reply(&request["id"], json!({"value": null, "version": 1}))
                    .await;
                doc_gets.fetch_add(1, Ordering::Relaxed);
            }
            _ => core.reply(&request["id"], Value::Null).await,
        }
    }
}

/// An app wired to in-memory opens plus the probe shared with `run`.
fn app(machines: Vec<Value>) -> (App, Arc<Probe>, Wires) {
    let (sub_tx, subs) = mpsc::channel(4);
    let (rpc_tx, rpcs) = mpsc::channel(4);
    let (attach_tx, attaches) = mpsc::channel(16);
    let (events_tx, events) = mpsc::channel(16);
    let doc_gets = Arc::new(AtomicU64::new(0));
    let watcher = Watcher::with_openers(
        rpc_opens(rpc_tx),
        loopcore::sub_opens(sub_tx),
        WatchParams {
            documents: vec!["herdr-app".into(), "hc-tui".into()],
            ..WatchParams::default()
        },
    );
    let config = hc_tui::config::Config::default();
    let shell = hc_tui::chrome::ClientShellConfig::from_config(
        &config,
        None,
        None,
        std::path::PathBuf::new(),
    );
    let probe = Arc::new(Probe::default());
    let app = App::new(
        watcher,
        loopcore::attach_opens(attach_tx),
        config.live_keybinds(),
        shell,
        probe.clone(),
    );
    (
        app,
        probe,
        Wires {
            subs,
            rpcs,
            attaches,
            events,
            events_tx,
            doc_gets,
            machines,
        },
    )
}

/// Everything the fake-core driver serves or sends.
struct Wires {
    subs: mpsc::Receiver<fakecore::DuplexCore>,
    rpcs: mpsc::Receiver<fakecore::DuplexCore>,
    attaches: mpsc::Receiver<(fakecore::DuplexCore, hc_proto::terminal::AttachParams)>,
    events: mpsc::Receiver<(String, Value)>,
    events_tx: mpsc::Sender<(String, Value)>,
    doc_gets: Arc<AtomicU64>,
    machines: Vec<Value>,
}

impl Wires {
    /// Serves conns forever: drains attaches, answers the rpc conn,
    /// replies the subscription snapshot then forwards `events`.
    fn spawn(mut self, snapshot: Value) -> mpsc::Sender<(String, Value)> {
        let events_tx = self.events_tx;
        let doc_gets = self.doc_gets;
        tokio::spawn(async move {
            let mut attaches = self.attaches;
            tokio::spawn(async move { while attaches.recv().await.is_some() {} });
            if let Some(rpc) = self.rpcs.recv().await {
                tokio::spawn(serve_rpc_conn(rpc, self.machines, doc_gets));
            }
            let mut subs = self.subs;
            let mut events = self.events;
            while let Some(mut sub) = subs.recv().await {
                sub.serve_plain_hello(loopcore::METHODS).await;
                let request = sub.read_message().await;
                assert_eq!(request["method"], "events.subscribe");
                sub.reply_line(&request["id"], snapshot.clone()).await;
                while let Some((method, params)) = events.recv().await {
                    sub.notify(&method, params).await;
                }
            }
        });
        events_tx
    }
}

/// Poll `doc_gets` until it reaches `want`.
async fn doc_gets_reach(doc_gets: &AtomicU64, want: u64) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while doc_gets.load(Ordering::Relaxed) < want {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("doc.get fetches never completed");
}

async fn run_app(app: &mut App, ui: &mut mpsc::Receiver<hc_tui::app::UiEvent>) {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let (_reply_tx, mut replies) = mpsc::channel(1);
    tokio::time::timeout(
        Duration::from_secs(20),
        app.run(&mut terminal, ui, &mut replies),
    )
    .await
    .expect("the loop must stay responsive")
    .unwrap();
}

#[tokio::test]
async fn startup_projects_at_most_once_per_endpoint() {
    const M: u64 = 3;
    let machines: Vec<Value> = ["host-a", "host-b", "host-c"]
        .iter()
        .map(|name| machine_json(name))
        .collect();
    let snapshot = json!({"seq": 1, "terminals": [
        loopcore::terminal_json("t1", "one", 1),
        loopcore::terminal_json("host-a/t1", "one", 1),
        loopcore::terminal_json("host-b/t1", "one", 1),
        loopcore::terminal_json("host-c/t1", "one", 1),
    ]});
    let (mut app, probe, wires) = app(machines);
    let events = wires.events_tx.clone();
    let doc_gets = wires.doc_gets.clone();
    wires.spawn(snapshot);
    let counters = probe.clone();
    let (ui_tx, mut ui) = mpsc::channel(8);
    let assertions = tokio::spawn(async move {
        // Local + M remotes × the two watched documents.
        doc_gets_reach(&doc_gets, 2 * (M + 1)).await;
        loopcore::settled_draws(&counters).await;
        let projections = counters.projections.load(Ordering::Relaxed);
        assert!(
            (1..=(M + 1)).contains(&projections),
            "startup ran {projections} projection rebuilds for {} endpoints",
            M + 1
        );
        let _ = events; // keep the notification lane open until done
        drop(ui_tx); // end the app loop
    });
    run_app(&mut app, &mut ui).await;
    assertions.await.unwrap();
}

#[tokio::test]
async fn no_op_publishes_run_zero_projections() {
    let snapshot = json!({"seq": 1, "terminals": [loopcore::terminal_json("t1", "one", 1)]});
    let (mut app, probe, wires) = app(Vec::new());
    let events = wires.events_tx.clone();
    let doc_gets = wires.doc_gets.clone();
    wires.spawn(snapshot);
    let counters = probe.clone();
    let (ui_tx, mut ui) = mpsc::channel(8);
    let assertions = tokio::spawn(async move {
        doc_gets_reach(&doc_gets, 2).await;
        loopcore::settled_draws(&counters).await;
        let baseline_projections = counters.projections.load(Ordering::Relaxed);
        let baseline_draws = counters.draws.load(Ordering::Relaxed);
        // An unwatched `doc` event: seq advances, nothing else changes.
        events
            .send((
                "event".into(),
                json!({"seq": 2, "type": "doc",
                "data": {"name": "unwatched"}}),
            ))
            .await
            .unwrap();
        let mut draws = loopcore::draws_beyond(&counters, baseline_draws).await;
        // A watched `doc` event: queues a `doc.get`; its null result
        // lands in another publish — still no projection input changed.
        events
            .send((
                "event".into(),
                json!({"seq": 3, "type": "doc",
                "data": {"name": "hc-tui"}}),
            ))
            .await
            .unwrap();
        draws = loopcore::draws_beyond(&counters, draws).await;
        doc_gets_reach(&doc_gets, 3).await;
        draws = loopcore::draws_beyond(&counters, draws).await;
        // A heartbeat: schedules `machine.list`, whose empty result is
        // another no-op publish.
        events
            .send(("heartbeat".into(), json!({"seq": 3})))
            .await
            .unwrap();
        loopcore::draws_beyond(&counters, draws).await;
        loopcore::settled_draws(&counters).await;
        let after = counters.projections.load(Ordering::Relaxed);
        assert_eq!(
            after,
            baseline_projections,
            "no-op publishes still ran {} projection rebuilds",
            after - baseline_projections
        );
        drop(ui_tx); // end the app loop
    });
    run_app(&mut app, &mut ui).await;
    assertions.await.unwrap();
}

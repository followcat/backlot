//! Damage-aware draw end to end (`r2-load-lag.md` R2-T4): with four
//! visible split panes, updating one pane costs about a quarter of the
//! four-pane cell work while the untouched panes keep their paint across
//! ratatui's buffer swap. `Probe::cells_painted` counts the VT cells
//! copied out per draw; a chrome-only redraw reads zero.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use hc_proto::watch::{OpenRpc, WatchParams, Watcher};
use hc_tui::app::{App, Probe};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};
use tokio::sync::mpsc;

#[path = "common/loopcore.rs"]
mod loopcore;
use loopcore::fakecore;

/// `OpenRpc` over fresh duplex pairs; the driver serves `machine.list`
/// and `doc.get` so the projection can lay out the four-pane tab.
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

/// The `herdr-app` structure document: one space, one tab, four blocks.
fn herdr_app_doc() -> Value {
    json!({
        "spaces": [{
            "id": "s1",
            "name": "Space",
            "folder": "/home/user",
            "tabs": [{
                "id": "tab1",
                "name": "Tab",
                "root": { "type": "row", "children": [
                    { "type": "block", "content": { "terminal": "t1" } },
                    { "type": "block", "content": { "terminal": "t2" } },
                    { "type": "block", "content": { "terminal": "t3" } },
                    { "type": "block", "content": { "terminal": "t4" } },
                ]},
            }],
        }],
    })
}

/// The `hc-tui` layout override: a 2x2 split, `t1` focused. The tab key
/// is `layout_key(None, "space:2:s1/tab:4:tab1")`.
fn tui_doc() -> Value {
    let pane = |t: &str| json!({ "type": "pane", "terminal": t });
    let half = |a: Value, b: Value| json!({ "type": "split", "vertical": false, "ratio": 0.5, "first": a, "second": b });
    json!({
        "schema": 1,
        "focus": "t1",
        "tabs": {
            "0:/space:2:s1/tab:4:tab1": {
                "root": {
                    "type": "split", "vertical": true, "ratio": 0.5,
                    "first": half(pane("t1"), pane("t2")),
                    "second": half(pane("t3"), pane("t4")),
                },
            },
        },
    })
}

/// Serves one rpc conn forever: empty `machine.list`, `doc.get` per
/// document name, `null` for anything else.
async fn serve_rpc_conn(mut core: fakecore::DuplexCore) {
    core.serve_hello(loopcore::METHODS).await;
    while let Some(request) = core.try_read_request().await {
        match request["method"].as_str().unwrap() {
            "machine.list" => {
                core.reply(&request["id"], json!({"machines": []})).await;
            }
            "doc.get" => {
                let value = match request["params"]["name"].as_str() {
                    Some("herdr-app") => herdr_app_doc(),
                    Some("hc-tui") => tui_doc(),
                    _ => Value::Null,
                };
                core.reply(&request["id"], json!({"value": value, "version": 1}))
                    .await;
            }
            _ => core.reply(&request["id"], Value::Null).await,
        }
    }
}

type Channels = (
    App,
    Arc<Probe>,
    mpsc::Receiver<fakecore::DuplexCore>,
    mpsc::Receiver<fakecore::DuplexCore>,
    mpsc::Receiver<(fakecore::DuplexCore, hc_proto::terminal::AttachParams)>,
);

fn app() -> Channels {
    let (sub_tx, subs) = mpsc::channel(4);
    let (rpc_tx, rpcs) = mpsc::channel(4);
    let (attach_tx, attaches) = mpsc::channel(16);
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
    let mut app = App::new(
        watcher,
        loopcore::attach_opens(attach_tx),
        config.live_keybinds(),
        shell,
        probe.clone(),
    );
    // These tests price pane paints: the opening page would cover the
    // panes and its one-time animation would add draws, so dismiss it.
    app.dismiss_opening();
    (app, probe, subs, rpcs, attaches)
}

fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

/// Wait until `cells_painted` strictly exceeds `more_than` — the draw
/// that read new VT cells has landed by definition.
async fn cells_beyond(probe: &Probe, more_than: u64) -> u64 {
    tokio::time::timeout(Duration::from_secs(5), async {
        while probe.cells_painted.load(Ordering::Relaxed) <= more_than {
            tokio::task::yield_now().await;
        }
        probe.cells_painted.load(Ordering::Relaxed)
    })
    .await
    .expect("an expected cell read never landed")
}

#[tokio::test]
async fn one_dirty_pane_costs_a_quarter_of_a_four_pane_paint() {
    let (mut app, probe, mut subs, mut rpcs, mut attaches) = app();
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    let (ui_tx, mut ui) = mpsc::channel(8);
    let (_reply_tx, mut replies) = mpsc::channel(1);
    let counters = probe.clone();
    let driver = tokio::spawn(async move {
        tokio::spawn(async move {
            if let Some(rpc) = rpcs.recv().await {
                serve_rpc_conn(rpc).await;
            }
        });
        // 1 active wire (t1), 3 observers, 4 size leases.
        let mut session = loopcore::Session::connect(
            &mut subs,
            &mut attaches,
            8,
            json!({
                "seq": 1,
                "terminals": [
                    loopcore::terminal_json("t1", "one", 1),
                    loopcore::terminal_json("t2", "one", 2),
                    loopcore::terminal_json("t3", "one", 3),
                    loopcore::terminal_json("t4", "one", 4),
                ],
            }),
        )
        .await;
        let t1 = session.active_wire("t1");
        let t2 = session.wire("t2", true);
        let t3 = session.wire("t3", true);
        let t4 = session.wire("t4", true);
        session.send_data(t1, b"\x1b[?9001lalpha").await;
        session.send_data(t2, b"\x1b[?9001lbravo").await;
        session.send_data(t3, b"\x1b[?9001lcharlie").await;
        session.send_data(t4, b"\x1b[?9001ldelta").await;
        loopcore::settled_draws(&counters).await;
        let four_pane_cells = counters.cells_painted.load(Ordering::Relaxed);
        assert!(four_pane_cells > 0, "initial paint read no cells");

        // A full repaint of t1 alone must cost about one pane's cells;
        // t2/t3/t4's VTs are not consulted at all.
        let before = counters.cells_painted.load(Ordering::Relaxed);
        session.send_data(t1, b"\x1b[2J\x1b[Halpha-v2").await;
        cells_beyond(&counters, before).await;
        loopcore::settled_draws(&counters).await;
        let one_dirty_delta = counters.cells_painted.load(Ordering::Relaxed) - before;
        assert!(
            one_dirty_delta * 3 <= four_pane_cells,
            "one dirty pane read {one_dirty_delta} cells of {four_pane_cells}"
        );

        // A one-line append reads only the touched rows of one pane —
        // never more than two inner rows across all four panes.
        // Inner width stays under half of a 120-column surface.
        let before = counters.cells_painted.load(Ordering::Relaxed);
        session.send_data(t1, b"\r\nline2").await;
        cells_beyond(&counters, before).await;
        loopcore::settled_draws(&counters).await;
        let partial_delta = counters.cells_painted.load(Ordering::Relaxed) - before;
        assert!(
            partial_delta <= 60 * 2,
            "partial append read {partial_delta} cells — more than two rows"
        );

        // A chrome-only draw (a rejection notice over untouched panes)
        // must not read a single VT cell.
        let before = counters.cells_painted.load(Ordering::Relaxed);
        let draws_before = counters.draws.load(Ordering::Relaxed);
        ui_tx
            .send(hc_tui::app::UiEvent::InputRejected("chrome only"))
            .await
            .unwrap();
        loopcore::draws_beyond(&counters, draws_before).await;
        let chrome_delta = counters.cells_painted.load(Ordering::Relaxed) - before;
        assert_eq!(chrome_delta, 0, "a chrome-only redraw read VT cells");

        drop(ui_tx);
    });
    tokio::time::timeout(
        Duration::from_secs(20),
        app.run(&mut terminal, &mut ui, &mut replies),
    )
    .await
    .expect("the loop must stay responsive")
    .unwrap();
    driver.await.unwrap();
    // The buffer survived ratatui's swap: the untouched panes kept
    // their paint through every draw in between.
    let text = buffer_text(&terminal);
    for marker in ["alpha-v2", "bravo", "charlie", "delta", "line2"] {
        assert!(
            text.contains(marker),
            "rendered buffer lost {marker}: {text}"
        );
    }
}

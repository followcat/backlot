//! Focused-pane attach dedup: the focused terminal's active wire paints
//! it, so a duplicate observer for the same terminal is never opened, a
//! stale one closes at first paint, and unfocusing hands the retained
//! screen to the observer cache instead of flashing blank.

use super::*;
use crate::testcore;
use hc_proto::client::Attach;
use hc_proto::frame::Kind;
use hc_proto::terminal::AttachParams;
use ratatui::backend::TestBackend;
use tokio::time::timeout;

fn attach_opens(accepted: mpsc::Sender<(testcore::FakeCore, AttachParams)>) -> OpenAttach {
    Arc::new(move |params| {
        let accepted = accepted.clone();
        Box::pin(async move {
            let (core, read, write) = testcore::pair(64 * 1024);
            accepted.send((core, params.clone())).await.unwrap();
            Attach::open(read, write, None, &params).await
        })
    })
}

/// Two panes split on one tab — the doc layout makes both `visible`.
fn two_pane_app() -> App {
    let (accepted, _rx) = mpsc::channel(1);
    let (rpc, sub) = crate::testcore::openers(accepted);
    let watcher = Watcher::with_openers(rpc, sub, hc_proto::watch::WatchParams::default());
    let open: OpenAttach = Arc::new(|_| Box::pin(std::future::pending()));
    let config = crate::config::Config::default();
    let shell = crate::chrome::ClientShellConfig::from_config(
        &config,
        None,
        None,
        std::path::PathBuf::new(),
    );
    let mut app = App::new(
        watcher,
        open,
        config.live_keybinds(),
        shell,
        Arc::new(Probe::default()),
    );
    app.set_plain(true);
    let terms: Vec<_> = ["t1", "t2"]
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let mut t = crate::sidebar::fixture::term(a);
            t.pid = Some(1);
            t.created_ms = i as u64;
            t.labels.insert("herdr.space".into(), "work".into());
            t.labels.insert("herdr.tab".into(), "tab".into());
            t
        })
        .collect();
    let mut view = View {
        link: hc_proto::watch::Link::Online,
        rpc: hc_proto::watch::Link::Online,
        core: Some(hc_proto::hello::CoreInfo {
            id: "id-host-a".into(),
            name: "host-a".into(),
            hostname: None,
        }),
        machines: Some(vec![]),
        ..Default::default()
    };
    view.inventory_mut().replace(1, terms);
    app.view = view;
    app.focused = Some("t1".into());
    app.refresh_projection();
    let tab = app.projection.snapshot.focused_tab_id.clone().unwrap();
    let key = crate::project::layout_key(None, &tab);
    app.view.documents_mut().insert(
        hc_proto::watch::DocKey {
            machine: None,
            name: "hc-tui".into(),
        },
        Arc::new(hc_proto::watch::Document {
            value: serde_json::json!({"schema":1,"tabs":{key:{"root":{
                "type":"split","vertical":true,"ratio":0.5,
                "first":{"type":"pane","terminal":"t1"},
                "second":{"type":"pane","terminal":"t2"}}}}}),
            version: 1,
            ..Default::default()
        }),
    );
    app.reconcile_published_view();
    app
}

/// Accept the next attach and serve its `terminal.attach` reply on
/// `channel`; returns the conn and the params it requested.
async fn serve(
    requests: &mut mpsc::Receiver<(testcore::FakeCore, AttachParams)>,
    app: &App,
    channel: u32,
) -> (testcore::FakeCore, AttachParams) {
    let (mut core, params) = timeout(testcore::DEADLINE, requests.recv())
        .await
        .expect("attach accept deadline")
        .expect("attach accept");
    let terminal = serde_json::to_value(app.view.inventory.get(&params.terminal).unwrap()).unwrap();
    core.serve_attach(testcore::attach_result(channel, terminal, true))
        .await;
    (core, params)
}

/// Accept `n` attaches, serve each on channel 7, and index them by
/// `(terminal, observe)` — attach accept order is unspecified.
async fn serve_all(
    requests: &mut mpsc::Receiver<(testcore::FakeCore, AttachParams)>,
    app: &App,
    n: usize,
) -> std::collections::HashMap<(String, bool), testcore::FakeCore> {
    let mut cores = std::collections::HashMap::new();
    for _ in 0..n {
        let (core, params) = serve(requests, app, 7).await;
        cores.insert((params.terminal, params.observe), core);
    }
    cores
}

async fn drive(app: &mut App) {
    let event = timeout(testcore::DEADLINE, app.pane_rx.recv())
        .await
        .expect("pane event deadline")
        .expect("pane event channel");
    app.pane_event(event, Instant::now());
}

/// The focused terminal rides its active attach — `sync_observers` must
/// not open a second, observe wire that would double its VT cost.
#[tokio::test]
async fn the_focused_pane_never_opens_a_duplicate_observer() {
    let mut app = two_pane_app();
    let (accepted, mut requests) = mpsc::channel(8);
    app.open_attach = attach_opens(accepted);
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    app.observers.clear(); // drop the fixture's pending-opener observers
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.sync_target();
    app.sync_observers();
    let mut seen = Vec::new();
    for _ in 0..2 {
        let (_core, params) = timeout(testcore::DEADLINE, requests.recv())
            .await
            .expect("attach accept deadline")
            .expect("attach accept");
        seen.push((params.terminal.clone(), params.observe));
    }
    seen.sort();
    assert_eq!(seen, [("t1".to_string(), false), ("t2".to_string(), true)]);
    assert!(
        timeout(std::time::Duration::from_millis(50), requests.recv())
            .await
            .is_err(),
        "a duplicate observer opened for the focused pane"
    );
    assert!(!app.observers.contains_key("t1"));
    assert!(app.observers.contains_key("t2"));
}

/// A focus switch keeps the destination's painted observer on screen
/// until the active wire finishes its first paint — then the duplicate
/// closes instead of feeding a second VT forever.
#[tokio::test]
async fn the_stale_observer_closes_once_the_focused_wire_paints() {
    let mut app = two_pane_app();
    let (accepted, mut requests) = mpsc::channel(16);
    app.open_attach = attach_opens(accepted);
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    app.observers.clear(); // drop the fixture's pending-opener observers
    app.focused = Some("t2".into());
    app.refresh_projection();
    app.sync_target();
    app.sync_observers();
    let mut cores = serve_all(&mut requests, &app, 2).await;
    cores
        .get_mut(&("t2".to_string(), false))
        .unwrap()
        .send_stream(7, Kind::Data, b"\x1b[?9001lt2 live")
        .await;
    timeout(testcore::DEADLINE, async {
        while !app.pane.has_fresh_paint() {
            drive(&mut app).await;
        }
    })
    .await
    .unwrap();
    cores
        .get_mut(&("t1".to_string(), true))
        .unwrap()
        .send_stream(7, Kind::Data, b"\x1b[?9001lt1 observed")
        .await;
    timeout(testcore::DEADLINE, async {
        while !app.observers["t1"].has_fresh_paint() {
            drive(&mut app).await;
        }
    })
    .await
    .unwrap();

    // Focus t2 -> t1: the painted observer t1 stays up while t1's
    // active wire attaches — never a blank pane.
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.sync_target();
    app.sync_observers();
    assert!(app.observers.contains_key("t1"), "must not flash blank");
    let shown = app.displayed_pane("t1").unwrap();
    assert!(
        !std::ptr::eq(shown, &app.pane),
        "the unpainted active wire must not displace the painted observer"
    );

    // The switch also opens t1's active wire and a fresh observer for
    // the just-unfocused t2.
    let mut switched = serve_all(&mut requests, &app, 2).await;
    switched
        .get_mut(&("t1".to_string(), false))
        .unwrap()
        .send_stream(7, Kind::Data, b"\x1b[?9001lt1 live")
        .await;
    timeout(testcore::DEADLINE, async {
        while !app.pane.has_fresh_paint() {
            drive(&mut app).await;
        }
    })
    .await
    .unwrap();
    assert!(!app.observers.contains_key("t1"));
    let shown = app.displayed_pane("t1").unwrap();
    assert!(std::ptr::eq(shown, &app.pane));
}

/// Unfocusing a pane whose wire is down hands its retained screen to a
/// fresh observer — the visible pane keeps its last paint, cursor and
/// palette state while the observe attach opens.
#[tokio::test]
async fn unfocusing_hands_the_retained_screen_to_a_fresh_observer() {
    let mut app = two_pane_app();
    let (accepted, mut requests) = mpsc::channel(16);
    app.open_attach = attach_opens(accepted);
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    app.observers.clear(); // drop the fixture's pending-opener observers
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.sync_target();
    app.sync_observers();
    let mut cores = serve_all(&mut requests, &app, 2).await;
    cores
        .get_mut(&("t1".to_string(), false))
        .unwrap()
        .send_stream(7, Kind::Data, b"\x1b[?9001lpainted t1")
        .await;
    timeout(testcore::DEADLINE, async {
        while !app.pane.has_fresh_paint() {
            drive(&mut app).await;
        }
    })
    .await
    .unwrap();
    // The wire dies: the last screen stays put while the pane is Down.
    app.pane_event(
        PaneEv::Ended {
            generation: app.pane.generation(),
        },
        Instant::now(),
    );
    assert!(app.pane.vt_ref().is_some());

    // Switch to t2: the handoff seeds a fresh t1 observer with that screen.
    app.focused = Some("t2".into());
    app.refresh_projection();
    app.sync_target();
    assert_eq!(app.pane.target(), Some("t2"));
    assert!(
        app.observers["t1"].vt_ref().is_some(),
        "the unfocused pane's last screen must hand over, not flash blank"
    );
    let text: String = app
        .observers
        .get_mut("t1")
        .unwrap()
        .vt()
        .unwrap()
        .frame(0)
        .unwrap()
        .rows
        .iter()
        .flat_map(|row| row.cells.iter().map(|cell| cell.text.as_str()))
        .collect();
    assert!(text.contains("painted t1"), "{text}");
    app.sync_observers();
    assert!(app.observers["t1"].vt_ref().is_some());
}

/// The focused pane's silent size lease stays even though its duplicate
/// observer is gone — the retain rule follows the active target too.
#[tokio::test]
async fn the_focused_panes_size_lease_survives_without_an_observer() {
    let mut app = two_pane_app();
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.sync_target();
    app.sync_observers();
    for _ in 0..2 {
        app.sched.mark_dirty(Instant::now());
        app.maybe_draw(&mut term, Instant::now()).unwrap();
    }
    assert!(app.size_leases.contains_key("t1"));
    assert!(app.size_leases.contains_key("t2"));
}

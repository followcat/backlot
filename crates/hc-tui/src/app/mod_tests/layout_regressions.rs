use super::*;
use crate::testcore::{self, FakeCore};
use hc_proto::{client::Attach, frame::Kind};

async fn painted(app: &mut App) -> Vec<FakeCore> {
    let (accepted, mut requests) = mpsc::channel(16);
    app.open_attach = Arc::new(move |params| {
        let accepted = accepted.clone();
        Box::pin(async move {
            let (core, read, write) = testcore::pair(65536);
            accepted.send((core, params.clone())).await.unwrap();
            Attach::open(read, write, None, &params).await
        })
    });
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.sync_observers();
    let mut peers = Vec::new();
    for _ in 0..app.observers.len() {
        let (mut core, params) = tokio::time::timeout(testcore::DEADLINE, requests.recv())
            .await
            .unwrap()
            .unwrap();
        let terminal =
            serde_json::to_value(app.view.inventory.get(&params.terminal).unwrap()).unwrap();
        core.serve_attach(testcore::attach_result(7, terminal, true))
            .await;
        let mut paint = b"\x1b[?9001l".to_vec();
        for n in 0..100 {
            paint.extend_from_slice(format!("line {n}\r\n").as_bytes());
        }
        core.send_stream(7, Kind::Data, &paint).await;
        peers.push(core);
    }
    tokio::time::timeout(testcore::DEADLINE, async {
        while app.observers.values().any(|p| !p.has_fresh_paint()) {
            let event = app.pane_rx.recv().await.unwrap();
            app.pane_event(event, Instant::now());
        }
    })
    .await
    .unwrap();
    app.pane = pane::Pane::new(
        app.pane_tx.clone(),
        Arc::new(|_| Box::pin(std::future::pending())),
    );
    peers
}

#[tokio::test]
async fn zoom_always_renders_the_terminal_receiving_input() {
    let mut app = app_with_spaces(&[("t1", "work"), ("t2", "work")]);
    app.focused = Some("t2".into());
    app.refresh_projection();
    let tab = app.projection.snapshot.focused_tab_id.clone().unwrap();
    let key = crate::project::layout_key(None, &tab);
    app.view.documents.insert(
        hc_proto::watch::DocKey {
            machine: None,
            name: "hc-tui".into(),
        },
        hc_proto::watch::Document {
            value: serde_json::json!({"schema":1,"tabs":{key:{"zoomed":"t2"}}}),
            version: 1,
            ..Default::default()
        },
    );
    app.refresh_projection();
    app.pane = pane::Pane::new(
        app.pane_tx.clone(),
        Arc::new(|_| Box::pin(std::future::pending())),
    );
    app.focus_tab(&tab);
    assert_eq!(app.focused.as_deref(), Some("t2"));
    app.focused = Some("t1".into());
    app.refresh_projection();
    assert_eq!(app.visible_panes(), ["t1"]);
    let infos =
        super::super::pane_geometry::pane_infos(&app, ratatui::layout::Rect::new(0, 0, 80, 24));
    assert_eq!(infos[0].id, "t1");
}

#[tokio::test]
async fn inventory_replacement_retires_cached_paint_before_delayed_old_close() {
    let mut app = app_with(&["t1"]);
    let _peers = painted(&mut app).await;
    let old = app.observers["t1"].generation();
    let mut terminal = app.view.inventory.get("t1").unwrap().clone();
    terminal.pid = Some(999);
    app.view.inventory.replace(1, vec![terminal]);
    app.sync_observers();
    assert_ne!(app.observers["t1"].generation(), old);
    assert!(!app.observers["t1"].has_fresh_paint());
    app.pane_event(
        PaneEv::Out {
            generation: old,
            out: hc_proto::client::Output::Data(b"old program".to_vec()),
        },
        Instant::now(),
    );
    assert!(!app.observers["t1"].has_fresh_paint());
}

#[tokio::test]
async fn scrollbar_drag_keeps_its_pane_when_focus_changes() {
    let mut app = app_with_spaces(&[("t1", "work"), ("t2", "work")]);
    let _peers = painted(&mut app).await;
    let _ = draw_to(&mut app, 120, 40);
    let hit = app
        .hit_map
        .panes
        .iter()
        .find(|h| h.pane_id == "t2")
        .unwrap()
        .clone();
    let track = hit.scrollbar_rect.unwrap();
    let event = |kind, col, row| {
        UiEvent::Mouse(super::super::input::MouseInput {
            kind,
            col,
            row,
            mods: hc_vt::Mods::NONE,
            sgr_position: None,
        })
    };
    use super::super::input::{MouseButton, MouseEventKind};
    app.on_input(
        event(MouseEventKind::Down(MouseButton::Left), track.x, track.y),
        120,
    );
    assert!(app.chrome_drag.is_some());
    app.focused = Some("t1".into());
    app.on_input(
        event(
            MouseEventKind::Drag(MouseButton::Left),
            hit.inner_rect.x + 1,
            hit.inner_rect.y + 5,
        ),
        120,
    );
    app.on_input(
        event(
            MouseEventKind::Up(MouseButton::Left),
            hit.inner_rect.x + 1,
            hit.inner_rect.y + 5,
        ),
        120,
    );
    assert!(app.chrome_drag.is_none());
    let a = app
        .observers
        .get_mut("t1")
        .unwrap()
        .vt()
        .unwrap()
        .scrollbar()
        .unwrap();
    let b = app
        .observers
        .get_mut("t2")
        .unwrap()
        .vt()
        .unwrap()
        .scrollbar()
        .unwrap();
    assert_eq!(a.offset, a.total - a.len);
    assert!(b.offset < b.total - b.len);
}

#[tokio::test]
async fn cached_observer_shutdown_shares_one_deadline() {
    let mut app = app_with_spaces(&[("t1", "work"), ("t2", "work"), ("t3", "work")]);
    let _unresponsive_peers = painted(&mut app).await;
    tokio::time::pause();
    let start = tokio::time::Instant::now();
    app.pane_rx.close();
    app.shutdown_panes().await;
    assert!(
        start.elapsed() <= std::time::Duration::from_secs(11),
        "each observer must not add another close timeout"
    );
    assert!(app.observers.is_empty());
}

#[tokio::test]
async fn visible_size_leases_survive_focus_and_leave_only_observers_off_tab() {
    let mut app = app_with_spaces(&[("t1", "work"), ("t2", "work"), ("t3", "other")]);
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let now = Instant::now();
    let mut draw = |app: &mut App, focus: &str, tick: u64| {
        app.focused = Some(focus.into());
        app.refresh_projection();
        app.sync_observers();
        let now = now + std::time::Duration::from_secs(tick);
        app.sched.mark_dirty(now);
        app.maybe_draw(&mut term, now).unwrap();
    };
    draw(&mut app, "t1", 0);
    assert_eq!(app.size_leases.len(), 2);
    let first = app.size_leases["t1"].generation();
    let second = app.size_leases["t2"].generation();
    draw(&mut app, "t2", 1);
    assert_eq!(app.size_leases["t1"].generation(), first);
    assert_eq!(app.size_leases["t2"].generation(), second);
    draw(&mut app, "t3", 2);
    assert_eq!(
        app.size_leases
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["t3"]
    );
    assert_eq!(app.observers.len(), 3);
}

#[tokio::test]
async fn zoomed_tab_keeps_covered_pane_lease_at_its_last_allocation() {
    let mut app = app_with_spaces(&[("t1", "work"), ("t2", "work")]);
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    app.focused = Some("t1".into());
    app.refresh_projection();
    let tab = app.projection.snapshot.focused_tab_id.clone().unwrap();
    let key = crate::project::layout_key(None, &tab);
    let split = serde_json::json!({"type":"split","vertical":true,"ratio":0.5,
        "first":{"type":"pane","terminal":"t1"},
        "second":{"type":"pane","terminal":"t2"}});
    let base = Instant::now() + std::time::Duration::from_secs(60);
    let draw = |app: &mut App, term: &mut Terminal<TestBackend>, tick: u64| {
        let now = base + std::time::Duration::from_secs(tick);
        app.sched.mark_dirty(now);
        app.maybe_draw(term, now).unwrap();
    };
    let doc = |app: &mut App, value: serde_json::Value, version: u64| {
        app.view.documents.insert(
            hc_proto::watch::DocKey {
                machine: None,
                name: "hc-tui".into(),
            },
            hc_proto::watch::Document {
                value,
                version,
                ..Default::default()
            },
        );
        app.reconcile_published_view();
    };
    doc(
        &mut app,
        serde_json::json!({"schema":1,"focus":"t1","tabs":{key.clone():{"root":split}}}),
        1,
    );
    draw(&mut app, &mut term, 0);
    assert_eq!(app.size_leases.len(), 2);
    let held = app.size_leases["t1"].allocated_size();
    let zoomed_split = app.size_leases["t2"].allocated_size();

    doc(
        &mut app,
        serde_json::json!({"schema":1,"focus":"t2","tabs":{key:{"root":split,"zoomed":"t2"}}}),
        2,
    );
    term.backend_mut().resize(80, 30);
    draw(&mut app, &mut term, 1);

    assert!(app.size_leases.contains_key("t1"));
    assert_eq!(app.size_leases["t1"].allocated_size(), held);
    assert_ne!(app.size_leases["t2"].allocated_size(), zoomed_split);
}

#[tokio::test]
async fn new_input_target_opens_at_its_current_geometry_before_any_draw() {
    let mut app = app_with(&["t1", "t2"]);
    let (tx, mut rx) = mpsc::channel(4);
    app.open_attach = Arc::new(move |params| {
        let tx = tx.clone();
        Box::pin(async move {
            tx.send(params).await.unwrap();
            std::future::pending().await
        })
    });
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    for (id, area) in [("t1", (65, 24)), ("t2", (120, 40))] {
        app.area = area;
        app.focused = Some(id.into());
        app.refresh_projection();
        let inner = pane_geometry::pane_inner(&mut app, area.0, area.1);
        app.sync_target();
        let params = tokio::time::timeout(crate::testcore::DEADLINE, rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(params.terminal, id);
        assert!(!params.observe);
        assert_eq!((params.cols, params.rows), (inner.width, inner.height));
        assert_ne!((params.cols, params.rows), (80, 24));
    }
}

#[tokio::test]
async fn dead_focus_falls_back_inside_its_tab_not_the_global_first() {
    // A command overlay's terminal can exit inventory a view before the
    // layout write that names the successor; the transient focus must not
    // land on another workspace — painting it would ack its completions.
    let mut app = app_with_spaces(&[("t1", "alpha"), ("t2", "beta"), ("t3", "beta")]);
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    app.focused = Some("t3".into());
    app.refresh_projection();
    assert_eq!(
        app.projection.snapshot.focused_pane_id.as_deref(),
        Some("t3")
    );

    let terms: Vec<_> = ["t1", "t2"]
        .iter()
        .map(|id| app.view.inventory.get(id.as_ref()).unwrap().clone())
        .collect();
    app.view.inventory.replace(2, terms);
    app.reconcile_published_view();

    assert_eq!(app.focused.as_deref(), Some("t2"));
}

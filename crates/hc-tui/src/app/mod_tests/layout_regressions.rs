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

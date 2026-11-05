use super::*;
use hc_proto::watch::{DocKey, Document};
use serde_json::json;

fn pending(app: &mut App) {
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
}

#[tokio::test]
async fn two_views_follow_shared_focus_zoom_and_split_geometry() {
    let mut a = app_with_spaces(&[("t1", "work"), ("t2", "work")]);
    let mut b = app_with_spaces(&[("t1", "work"), ("t2", "work")]);
    pending(&mut a);
    pending(&mut b);
    a.focused = Some("t1".into());
    a.refresh_projection();
    let key =
        crate::project::layout_key(None, a.projection.snapshot.focused_tab_id.as_ref().unwrap());
    let value = json!({"schema":1,"focus":"t2","tabs":{key:{"root":{"type":"split","vertical":true,"ratio":0.25,"first":{"type":"pane","terminal":"t1"},"second":{"type":"pane","terminal":"t2"}},"zoomed":"t2"}}});
    for app in [&mut a, &mut b] {
        app.view.documents.insert(
            DocKey {
                machine: None,
                name: "hc-tui".into(),
            },
            Document {
                value: value.clone(),
                version: 2,
                ..Default::default()
            },
        );
        app.reconcile_published_view();
        assert_eq!(app.focused.as_deref(), Some("t2"));
        assert_eq!(app.visible_panes(), ["t2"]);
        assert_eq!(app.observers.len(), 2, "zoom keeps all tab panes attached");
        let infos =
            super::super::pane_geometry::pane_infos(app, ratatui::layout::Rect::new(0, 0, 80, 24));
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].rect.width, 80);
    }
}

#[tokio::test]
async fn recent_observers_keep_their_wire_and_evict_the_oldest_off_tab() {
    let ids = (1..=36).map(|n| format!("t{n}")).collect::<Vec<_>>();
    let refs = ids.iter().map(String::as_str).collect::<Vec<_>>();
    let mut app = app_with(&refs);
    pending(&mut app);
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.sync_observers();
    let route = app.observers["t1"].generation();
    for id in &ids[1..33] {
        app.focused = Some(id.clone());
        app.refresh_projection();
        app.sync_observers();
    }
    assert_eq!(app.observers.len(), 33);
    assert_eq!(app.observers["t1"].generation(), route);
    app.focused = Some("t34".into());
    app.refresh_projection();
    app.sync_observers();
    assert_eq!(app.observers.len(), 33);
    assert!(!app.observers.contains_key("t1"));
    assert!(app.observers.contains_key("t2"));
}

#[tokio::test]
async fn cached_observer_keeps_one_shot_cursor_and_palette_across_switching() {
    use crate::testcore;
    use hc_proto::{client::Attach, frame::Kind};
    let mut app = app_with(&["t1", "t2"]);
    let (accepted, mut requests) = mpsc::channel(4);
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
    let (mut core, params) = tokio::time::timeout(testcore::DEADLINE, requests.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(params.observe);
    let terminal = serde_json::to_value(crate::sidebar::fixture::term("t1")).unwrap();
    core.serve_attach(testcore::attach_result(7, terminal, true))
        .await;
    core.send_stream(
        7,
        Kind::Data,
        b"\x1b[?9001l\x1b[5 q\x1b]12;#123456\x07\x1b]4;1;#345678\x07\x1b[31mcache",
    )
    .await;
    tokio::time::timeout(testcore::DEADLINE, async {
        while !app.observers["t1"].has_fresh_paint() {
            let event = app.pane_rx.recv().await.unwrap();
            app.pane_event(event, Instant::now());
        }
    })
    .await
    .unwrap();
    let before = app
        .observers
        .get_mut("t1")
        .unwrap()
        .vt()
        .unwrap()
        .frame(0)
        .unwrap();
    let route = app.observers["t1"].generation();
    app.focused = Some("t2".into());
    app.refresh_projection();
    app.sync_observers();
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.sync_observers();
    assert_eq!(app.observers["t1"].generation(), route);
    let after = app
        .displayed_pane_mut("t1")
        .unwrap()
        .vt()
        .unwrap()
        .frame(0)
        .unwrap();
    assert_eq!(before.cursor.shape, hc_vt::CursorShape::Bar);
    assert_eq!(after.cursor.shape, before.cursor.shape);
    assert_eq!(after.colors.cursor, before.colors.cursor);
    assert_eq!(after.colors.palette[1], before.colors.palette[1]);
    // Cached observers can grant output credit, but never input, focus, facts
    // or a resize, even after a program asks for a response.
    core.send_stream(7, Kind::Data, b"\x1b[c").await;
    let event = tokio::time::timeout(testcore::DEADLINE, app.pane_rx.recv())
        .await
        .unwrap()
        .unwrap();
    app.pane_event(event, Instant::now());
    while let Ok(Some(frame)) =
        tokio::time::timeout(std::time::Duration::from_millis(20), core.try_read_frame()).await
    {
        assert_eq!(frame.kind, Kind::Credit);
    }
}

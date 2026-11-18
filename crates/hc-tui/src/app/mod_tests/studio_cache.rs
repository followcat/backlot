//! The studio's role terminals are ordinary panes: a studio switch asks
//! for the configured cold-open history, hands the VT it leaves to the
//! observer that inherits the terminal, continues that VT on a switch
//! back, and never leaves the previous workspace's screen on the pane.

use super::pane_cache::{drive_until, next_attach, serve, with_openers};
use super::*;
use crate::config::DEFAULT_ATTACH_HISTORY_LINES;
use crate::studio::StudioKey;
use hc_proto::frame::Kind;
use hc_proto::watch::{DocKey, Document};

/// The screen-only replay the core sends for a warm attach.
const WARM: &[u8] = b"\x18\x1b[!p\x1b[H\x1b[2J";

/// One studio (`acme-shop`) whose producer is `t1`, beside one ordinary
/// terminal `t2` in its own workspace.
fn studio_app() -> App {
    let mut app = app_with(&["t1", "t2"]);
    app.set_plain(false);
    let producer = serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "studios": [{
            "id": "acme-shop", "title": "Acme shop", "state": "live",
            "requirement_version": 1, "mode": "standard",
            "open_reviews": 0, "waiting": 0,
            "roles": [{ "role": "producer", "name": "producer", "terminal": "t1",
                        "state": "live", "work": "working" }]
        }]
    });
    app.view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        Document {
            value: producer,
            ..Document::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    app
}

/// What the real loop does after a focus change: reconcile the pane and
/// open the observers for the tab it just left.
fn settle(app: &mut App) {
    let view = app.view.clone();
    app.pane.on_view(&view);
    app.sync_observers();
}

/// Screen plus scrollback as plain text.
fn shown(app: &App) -> String {
    let Some(vt) = app.pane.vt_ref() else {
        return String::new();
    };
    let rows = vt.total_rows().unwrap();
    vt.screen_text_rows_range(0, rows)
        .unwrap()
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .filter_map(|cell| cell.graphemes.first().and_then(|g| char::from_u32(*g)))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn a_studio_role_terminal_keeps_its_screen_across_a_workspace_switch() {
    let mut app = studio_app();
    let mut requests = with_openers(&mut app);

    // The studio's own switch to its producer (t1).
    app.focus_studio(&StudioKey::local("acme-shop"));
    settle(&mut app);
    assert_eq!(app.focused.as_deref(), Some("t1"));
    let (core1, params1) = next_attach(&mut app, &mut requests).await;
    assert_eq!(params1.terminal, "t1");
    assert!(!params1.observe);
    assert_eq!(
        params1.history, DEFAULT_ATTACH_HISTORY_LINES,
        "a studio terminal opens with the configured history"
    );
    let mut core1 = serve(core1, &params1, 1).await;
    core1
        .send_stream(1, Kind::Data, b"\x1b[?9001lproducer screen")
        .await;
    drive_until(&mut app, |app| app.pane.has_fresh_paint()).await;
    let t1_vt = app.pane.vt_ref().unwrap().id();

    // To an ordinary workspace: the producer's screen goes to the
    // observer, and the ordinary pane paints its own screen.
    app.focus_pane("t2".into());
    settle(&mut app);
    let (observer, observer_params) = next_attach(&mut app, &mut requests).await;
    assert_eq!(observer_params.terminal, "t1");
    assert!(observer_params.observe);
    assert_eq!(
        observer_params.history, 0,
        "leaving must not download history"
    );
    let mut observer = serve(observer, &observer_params, 2).await;
    let (core2, params2) = next_attach(&mut app, &mut requests).await;
    assert_eq!(params2.terminal, "t2");
    assert!(!params2.observe);
    assert_eq!(params2.history, DEFAULT_ATTACH_HISTORY_LINES);
    let mut core2 = serve(core2, &params2, 3).await;
    core2
        .send_stream(3, Kind::Data, b"\x1b[?9001lplain screen")
        .await;
    let mut warm = WARM.to_vec();
    warm.extend_from_slice(b"producer warm\x1b[?9001l");
    observer.send_stream(2, Kind::Data, &warm).await;
    drive_until(&mut app, |app| app.pane.has_fresh_paint()).await;
    drive_until(&mut app, |app| {
        app.observers
            .get("t1")
            .is_some_and(|pane| pane.has_fresh_paint())
    })
    .await;
    let text = shown(&app);
    assert!(text.contains("plain screen"), "{text}");
    assert!(
        !text.contains("producer"),
        "the studio's screen must not stay on an ordinary workspace: {text}"
    );

    // Back through the studio: the producer's VT continues.
    app.focus_studio(&StudioKey::local("acme-shop"));
    settle(&mut app);
    assert_eq!(app.focused.as_deref(), Some("t1"));
    let (t2_observer, t2_observer_params) = next_attach(&mut app, &mut requests).await;
    assert_eq!(t2_observer_params.terminal, "t2");
    assert!(t2_observer_params.observe);
    let _t2_observer = serve(t2_observer, &t2_observer_params, 4).await;
    let (back, back_params) = next_attach(&mut app, &mut requests).await;
    assert_eq!(back_params.terminal, "t1");
    assert!(!back_params.observe);
    assert_eq!(
        back_params.history, 0,
        "a warm switch asks for the screen only"
    );
    let mut back = serve(back, &back_params, 5).await;
    back.send_stream(
        5,
        Kind::Data,
        &[WARM, b"producer again\x1b[?9001l"].concat(),
    )
    .await;
    drive_until(&mut app, |app| app.pane.has_fresh_paint()).await;
    assert_eq!(
        app.pane.vt_ref().map(|vt| vt.id()),
        Some(t1_vt),
        "the studio terminal continued its cached VT"
    );
    assert!(shown(&app).contains("producer again"));
}

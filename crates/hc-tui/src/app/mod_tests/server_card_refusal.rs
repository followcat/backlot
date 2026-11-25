//! Fail-first and regression tests for server-mode card approval refusal
//! in the Call Sheet, Lot Reader, and context menus while local cards keep working.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use backlot_model::req_wire::{CAT_DOC_NAME, REQ_DOC_NAME};
use hc_proto::watch::{DocKey, Document};
use serde_json::json;

use super::server_doc_fake::{MemDocs, catalog_json, spawn_doc_core_with};
use super::*;
use crate::hollywood::{CardView, LayerMode, Target};
use crate::studio_menu::{StudioAct, StudioMenu};

const EXPECTED_REFUSAL: &str =
    "card approvals are not taken over the server queue; apply it on the server host";

fn card_summary_json() -> serde_json::Value {
    json!({
        "schema": "backlot/1",
        "updated_ms": 2,
        "studios": [{
            "id": "acme-shop",
            "title": "Acme shop",
            "state": "live",
            "requirement_version": 1,
            "mode": "standard",
            "open_reviews": 0,
            "waiting": 1,
            "roles": [{
                "role": "producer",
                "name": "producer",
                "terminal": "t1",
                "state": "live",
                "work": "working"
            }]
        }],
        "hollywood": {
            "state": "live",
            "runner": "fake",
            "model": "model-h",
            "effort": "high",
            "headline": "acme-shop needs a look",
            "cards": [{
                "id": "c3",
                "studio": "acme-shop",
                "action": "pause t1",
                "why": "actor is looping"
            }]
        }
    })
}

fn server_app_with_card() -> (App, Arc<Mutex<MemDocs>>) {
    let store = Arc::new(Mutex::new(MemDocs::default()));
    {
        let mut st = store.lock().unwrap();
        st.docs.insert("backlot".into(), (1, card_summary_json()));
        st.docs.insert(CAT_DOC_NAME.into(), (1, catalog_json(true)));
    }
    let (rpc, _) = spawn_doc_core_with(store.clone(), None);
    let mut app = app_with(&[]);
    app.set_plain(false);
    app.view.link = hc_proto::watch::Link::Online;
    app.view.rpc = hc_proto::watch::Link::Online;
    app.view.core = Some(hc_proto::hello::CoreInfo {
        id: "core-srv".into(),
        name: "backlot".into(),
        hostname: None,
    });
    app.set_operations(rpc);
    app.set_server_target(Some(backlot_stage::server_cfg::ServerTarget {
        host: "host-a".into(),
        session: "backlot".into(),
    }));
    app.server_ack_timeout = Duration::from_millis(60);
    app.view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        Document {
            value: card_summary_json(),
            version: 1,
            ..Document::default()
        }
        .into(),
    );
    app.view.documents_mut().insert(
        DocKey {
            machine: None,
            name: CAT_DOC_NAME.into(),
        },
        Document {
            value: catalog_json(true),
            version: 1,
            ..Document::default()
        }
        .into(),
    );
    let mut srv = crate::sidebar::fixture::term("t9");
    srv.name = Some("backlot-server".into());
    srv.state = "live".into();
    app.view.inventory_mut().replace(1, vec![srv]);
    app.area = (120, 40);
    app.reconcile_published_view();
    (app, store)
}

fn send_key(app: &mut App, code: hc_vt::Key) {
    app.on_input(
        input::UiEvent::Key(term_key(code, hc_vt::Mods::NONE, hc_vt::Action::Press)),
        120,
    );
}

#[tokio::test]
async fn server_call_sheet_and_menu_refuse_cards_without_spinner_or_req_write() {
    let (mut app, store) = server_app_with_card();
    app.hollywood.mode = LayerMode::Expanded;
    app.hollywood.selected = Some(Target::Card("c3".into()));

    send_key(&mut app, hc_vt::Key::Char('a'));
    assert!(app.hollywood.pending.is_none(), "no spinner when refused");
    assert!(app.actions.pending.is_empty(), "no action queued");
    assert!(app.action_toast.is_none());
    assert!(!store.lock().unwrap().docs.contains_key(REQ_DOC_NAME));
    let screen_a = screen_lines(&draw_to(&mut app, 120, 40)).join("\n");
    assert!(screen_a.contains(EXPECTED_REFUSAL), "{screen_a}");

    app.notice = None;
    send_key(&mut app, hc_vt::Key::Char('x'));
    assert!(
        app.studio_dialog.is_none(),
        "cancel dialog must not open on refused server card"
    );
    assert!(app.hollywood.pending.is_none());
    assert!(app.actions.pending.is_empty());
    assert!(!store.lock().unwrap().docs.contains_key(REQ_DOC_NAME));
    let screen_x = screen_lines(&draw_to(&mut app, 120, 40)).join("\n");
    assert!(screen_x.contains(EXPECTED_REFUSAL), "{screen_x}");

    // Studio card menu activation also routes through the same guard.
    app.notice = None;
    app.run_studio_act(&StudioMenu::card("c3"), StudioAct::CardApply);
    assert!(app.actions.pending.is_empty());
    assert!(!store.lock().unwrap().docs.contains_key(REQ_DOC_NAME));
    let screen_menu = screen_lines(&draw_to(&mut app, 120, 40)).join("\n");
    assert!(screen_menu.contains(EXPECTED_REFUSAL), "{screen_menu}");
}

#[tokio::test]
async fn server_card_in_reader_shows_refusal_in_action_strip_without_spinner_or_req_write() {
    let (mut app, store) = server_app_with_card();
    app.hollywood.mode = LayerMode::Expanded;
    app.hollywood.view = Some(CardView {
        card: Some("c3".into()),
        title: "c3".into(),
        lines: vec!["pause t1 — actor is looping".into()],
        scroll: 0,
        ..CardView::default()
    });

    send_key(&mut app, hc_vt::Key::Char('a'));
    assert!(app.hollywood.view.is_some());
    assert!(app.hollywood.pending.is_none());
    assert!(app.actions.pending.is_empty());
    assert!(!store.lock().unwrap().docs.contains_key(REQ_DOC_NAME));
    let reader_a = screen_lines(&draw_to(&mut app, 120, 40)).join("\n");
    assert!(reader_a.contains(EXPECTED_REFUSAL), "{reader_a}");
    assert!(!reader_a.contains("a/x locked"), "{reader_a}");

    app.notice = None;
    app.hollywood.view = Some(CardView {
        card: Some("c3".into()),
        title: "c3".into(),
        lines: vec!["pause t1 — actor is looping".into()],
        scroll: 0,
        ..CardView::default()
    });
    send_key(&mut app, hc_vt::Key::Char('x'));
    assert!(app.studio_dialog.is_none());
    assert!(app.hollywood.pending.is_none());
    assert!(app.actions.pending.is_empty());
    assert!(!store.lock().unwrap().docs.contains_key(REQ_DOC_NAME));
    let reader_x = screen_lines(&draw_to(&mut app, 120, 40)).join("\n");
    assert!(reader_x.contains(EXPECTED_REFUSAL), "{reader_x}");
    assert!(!reader_x.contains("a/x locked"), "{reader_x}");
}

#[tokio::test]
async fn local_card_still_applies_directly_when_not_in_server_mode() {
    let mut app = app_with(&["t1"]);
    app.set_plain(false);
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    app.view.inventory_mut().replace(1, vec![stage]);
    app.view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        Document {
            value: card_summary_json(),
            version: 1,
            ..Document::default()
        }
        .into(),
    );
    app.area = (120, 40);
    app.reconcile_published_view();

    let recorded = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_hollywood_runner(Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(String::new()) })
    }));

    app.hollywood.mode = LayerMode::Expanded;
    app.hollywood.selected = Some(Target::Card("c3".into()));
    send_key(&mut app, hc_vt::Key::Char('a'));

    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec!["card", "apply", "c3", "--as", "user"]]
    );
    assert!(app.notice.is_none());
}

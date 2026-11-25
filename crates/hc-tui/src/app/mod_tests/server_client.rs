//! Tests for B2 TUI server endpoint, catalog reads, `ActionTracker` via
//! `plugin.backlot.req`/`res`, and stopped-server notice.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use backlot_model::req_results::TellResult;
use backlot_model::req_wire::{
    CAT_DOC_NAME, MAX_REQ_QUEUE, REQ_DOC_NAME, REQ_SCHEMA, ReqAction, ReqDoc, ReqEnvelope, ResAck,
};
use hc_proto::watch::{DocKey, Document};
use serde_json::{Value, json};

use super::server_doc_fake::{MemDocs, catalog_json, spawn_doc_core_with, summary_json};
use super::*;
use crate::studio::StudioKey;
use crate::studio_cmds;
use crate::testcore;

fn empty_online_app() -> App {
    let mut app = app_with(&[]);
    app.set_plain(false);
    app.view.link = hc_proto::watch::Link::Online;
    app.view.rpc = hc_proto::watch::Link::Online;
    app.view.core = Some(hc_proto::hello::CoreInfo {
        id: "core-srv".into(),
        name: "backlot".into(),
        hostname: None,
    });
    app
}

#[test]
fn server_terminal_and_reap_guard_recognize_backlot_server() {
    let mut term = crate::sidebar::fixture::term("t9");
    term.name = Some("backlot-server".into());
    term.state = "live".into();

    assert!(crate::panel::data::is_stage_terminal(&term));
    let mut view = hc_proto::watch::View::default();
    view.inventory_mut().replace(1, vec![term.clone()]);
    assert!(crate::panel::stage_running(&view, None));

    term.state = "finished".into();
    term.reason = Some("exited".into());
    assert!(!crate::app::reap::should_reap_exited(&term, 10_000));
}

#[tokio::test]
async fn stopped_server_with_catalog_shows_command_and_never_starts_stage() {
    let mut app = empty_online_app();
    app.area = (120, 40);
    app.view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        Document {
            value: Value::Null,
            version: 0,
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
    app.reconcile_published_view();
    assert!(app.has_hollywood_cfg);

    let recorded = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_panel_runner(Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(()) })
    }));

    let out = screen_lines(&draw_to(&mut app, 120, 40)).join("\n");
    assert!(
        out.contains("systemctl --user start backlot-server.service")
            || out.contains("backlot server start --session backlot"),
        "{out}"
    );
    assert!(!out.contains("stage stopped · s start"), "{out}");
    assert!(!out.contains("Start the stage"), "{out}");

    app.on_input(
        input::UiEvent::Key(term_key(
            hc_vt::Key::Char('s'),
            hc_vt::Mods::NONE,
            hc_vt::Action::Press,
        )),
        120,
    );
    assert!(recorded.lock().unwrap().is_empty());

    app.view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        Document {
            value: summary_json(),
            version: 1,
            ..Document::default()
        }
        .into(),
    );
    app.panel.mode = crate::panel::PanelMode::Expanded;
    app.hollywood.mode = crate::hollywood::LayerMode::Expanded;
    app.reconcile_published_view();

    let with_studios = screen_lines(&draw_to(&mut app, 120, 40)).join("\n");
    assert!(
        with_studios.contains("systemctl --user start backlot-server.service"),
        "{with_studios}"
    );
    assert!(
        !with_studios.contains("stage stopped · s start"),
        "{with_studios}"
    );
    assert_eq!(
        app.panel.server_stopped_cmd.as_deref(),
        Some("backlot server start --session backlot")
    );

    app.panel.focused = true;
    app.on_input(
        input::UiEvent::Key(term_key(
            hc_vt::Key::Char('s'),
            hc_vt::Mods::NONE,
            hc_vt::Action::Press,
        )),
        120,
    );
    app.panel.focused = false;
    assert!(recorded.lock().unwrap().is_empty());
}

#[tokio::test]
async fn server_action_sends_b1a_and_formats_ok_busy_expired_missing_and_unsupported() {
    let store = Arc::new(Mutex::new(MemDocs::default()));
    let (rpc, _) = spawn_doc_core_with(store.clone(), None);
    let run_it = |action: crate::action::Action, timeout: Duration| {
        crate::server_action::run(
            Some(rpc.clone()),
            None,
            action.argv,
            action.stdin,
            Some("acme-shop".into()),
            timeout,
        )
    };
    let key = StudioKey::local("acme-shop");

    let err = run_it(
        studio_cmds::finish(&key, Some("done")),
        Duration::from_secs(1),
    )
    .await
    .unwrap_err();
    assert_eq!(err, "not available through the server yet");
    assert!(!store.lock().unwrap().docs.contains_key(REQ_DOC_NAME));

    store.lock().unwrap().auto_ack = Some(ResAck::ok_value(
        "pending",
        200,
        serde_json::to_value(TellResult {
            studio: "acme-shop".into(),
            role: "producer".into(),
            seq: 11,
            file: "messages/11.md".into(),
        })
        .unwrap(),
    ));
    let ok = run_it(
        studio_cmds::tell(&key, "producer", "hello via doc"),
        Duration::from_secs(1),
    )
    .await
    .unwrap();
    assert!(ok.contains("message.queued"), "{ok}");

    let req_val = store
        .lock()
        .unwrap()
        .docs
        .get(REQ_DOC_NAME)
        .unwrap()
        .1
        .clone();
    let req_doc: ReqDoc = serde_json::from_value(req_val).unwrap();
    let env: ReqEnvelope = serde_json::from_value(req_doc.queue.last().unwrap().clone()).unwrap();
    let req_act: ReqAction = serde_json::from_value(env.action).unwrap();
    assert!(matches!(
        req_act,
        ReqAction::Tell { studio, role, text }
            if studio == "acme-shop" && role == "producer" && text == "hello via doc"
    ));

    store.lock().unwrap().docs.insert(
        REQ_DOC_NAME.into(),
        (10, json!({ "schema": REQ_SCHEMA, "queue": [] })),
    );
    let err_missing = run_it(
        studio_cmds::tell(&key, "producer", "no reply"),
        Duration::from_millis(60),
    )
    .await
    .unwrap_err();
    assert!(!err_missing.contains('\n'), "{err_missing}");
    assert!(err_missing.contains("timed out"), "{err_missing}");

    let q: Vec<Value> = (0..MAX_REQ_QUEUE)
        .map(|i| json!({ "req_id": format!("r{i}"), "ts_ms": 1 }))
        .collect();
    store.lock().unwrap().docs.insert(
        REQ_DOC_NAME.into(),
        (99, json!({ "schema": REQ_SCHEMA, "queue": q })),
    );
    assert_eq!(
        run_it(
            studio_cmds::priority(&key, "t1", "high"),
            Duration::from_secs(1)
        )
        .await
        .unwrap_err(),
        "server busy"
    );

    {
        let mut st = store.lock().unwrap();
        st.docs.insert(
            REQ_DOC_NAME.into(),
            (100, json!({ "schema": REQ_SCHEMA, "queue": [] })),
        );
        st.auto_ack = Some(ResAck::err("pending", 300, "request expired"));
    }
    assert_eq!(
        run_it(studio_cmds::close(&key, "writer"), Duration::from_secs(1))
            .await
            .unwrap_err(),
        "request expired"
    );
}

#[tokio::test]
async fn app_action_tracker_runs_via_doc_queue_and_watcher_follows_docs() {
    let store = Arc::new(Mutex::new(MemDocs::default()));
    {
        let mut st = store.lock().unwrap();
        st.docs.insert("backlot".into(), (1, summary_json()));
        st.docs.insert(CAT_DOC_NAME.into(), (1, catalog_json(true)));
    }
    let sub_notify = Arc::new(tokio::sync::Notify::new());
    let (rpc, sub) = spawn_doc_core_with(store.clone(), Some(sub_notify.clone()));

    let watcher = hc_proto::watch::Watcher::with_openers(
        rpc.clone(),
        sub,
        crate::cli::watch_params_for(true),
    );
    let backlot_key = DocKey {
        machine: None,
        name: "backlot".into(),
    };
    let cat_key = DocKey {
        machine: None,
        name: CAT_DOC_NAME.into(),
    };
    let view1 = testcore::await_view(&watcher, |v| {
        v.documents
            .get(&backlot_key)
            .is_some_and(|d| !d.fetching && !d.value.is_null())
            && v.documents
                .get(&cat_key)
                .is_some_and(|d| !d.fetching && !d.value.is_null())
    })
    .await;
    assert_eq!(view1.documents[&backlot_key].version, 1);

    let mut v2 = summary_json();
    v2["studios"][0]["title"] = json!("Shop Updated");
    store.lock().unwrap().docs.insert("backlot".into(), (2, v2));
    sub_notify.notify_one();
    let view2 = testcore::await_view(&watcher, |v| {
        v.documents
            .get(&backlot_key)
            .is_some_and(|d| d.version == 2)
    })
    .await;
    assert_eq!(
        view2.documents[&backlot_key].value["studios"][0]["title"],
        "Shop Updated"
    );

    let mut app = empty_online_app();
    app.view = view2;
    app.set_operations(rpc);
    app.set_server_target(Some(backlot_stage::server_cfg::ServerTarget {
        host: "host-a".into(),
        session: "backlot".into(),
    }));
    app.server_ack_timeout = Duration::from_millis(60);
    app.sync_studio();
    assert!(app.has_hollywood_cfg);

    let key = StudioKey::local("acme-shop");
    store.lock().unwrap().auto_ack = Some(ResAck::ok_value(
        "pending",
        200,
        serde_json::to_value(TellResult {
            studio: "acme-shop".into(),
            role: "producer".into(),
            seq: 12,
            file: "messages/12.md".into(),
        })
        .unwrap(),
    ));
    app.run_action(studio_cmds::tell(&key, "producer", "hi via tracker"));
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    let toast = app.action_toast.as_ref().unwrap();
    assert!(!toast.fail, "{}", toast.body);
    assert_eq!(toast.start_stage, None);

    app.run_action(studio_cmds::tell(&key, "producer", "no ack"));
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    let toast = app.action_toast.as_ref().unwrap();
    assert!(toast.fail);
    assert!(toast.body.contains("timed out"), "{}", toast.body);

    app.run_action(studio_cmds::finish(&key, Some("done")));
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    let toast = app.action_toast.as_ref().unwrap();
    assert!(toast.fail);
    assert_eq!(toast.body, "not available through the server yet");
}

fn last_queued_action(store: &Mutex<MemDocs>) -> ReqAction {
    let st = store.lock().unwrap();
    let (_, val) = st.docs.get(REQ_DOC_NAME).expect("req doc written").clone();
    let doc: ReqDoc = serde_json::from_value(val).unwrap();
    let env: ReqEnvelope = serde_json::from_value(doc.queue.last().unwrap().clone()).unwrap();
    serde_json::from_value(env.action).unwrap()
}

#[tokio::test]
async fn server_mode_card_apply_and_cancel_send_requests_over_the_doc_queue() {
    use backlot_model::req_results::{CardApplyResult, CardCancelResult};

    let store = Arc::new(Mutex::new(MemDocs::default()));
    let (rpc, _) = spawn_doc_core_with(store.clone(), None);
    let mut app = empty_online_app();
    app.set_server_target(Some(backlot_stage::server_cfg::ServerTarget {
        host: "host-a".into(),
        session: "backlot".into(),
    }));
    app.server_ack_timeout = Duration::from_secs(1);
    app.set_operations(rpc);
    assert!(app.server_mode());

    store.lock().unwrap().auto_ack = Some(ResAck::ok_value(
        "pending",
        200,
        serde_json::to_value(CardApplyResult {
            id: "c3".into(),
            seq: 3,
            outcome_seq: 7,
            stale: None,
            summary: "event 7: card.applied c3 — carrying the action out is the stage's job (H3)"
                .into(),
        })
        .unwrap(),
    ));
    app.run_action(studio_cmds::card_apply_on(None, "c3"));
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    let toast = app.action_toast.as_ref().unwrap();
    assert!(!toast.fail, "{}", toast.body);
    assert!(matches!(
        last_queued_action(&store),
        ReqAction::CardApply { id } if id == 3
    ));

    store.lock().unwrap().auto_ack = Some(ResAck::ok_value(
        "pending",
        300,
        serde_json::to_value(CardCancelResult {
            id: "c4".into(),
            seq: 4,
            outcome_seq: 8,
            reason: "hold".into(),
        })
        .unwrap(),
    ));
    app.run_action(studio_cmds::card_cancel_on(None, "c4", Some("hold")));
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    let toast = app.action_toast.as_ref().unwrap();
    assert!(!toast.fail, "{}", toast.body);
    assert!(matches!(
        last_queued_action(&store),
        ReqAction::CardCancel { id, reason } if id == 4 && reason == "hold"
    ));
}

//! Revise requirement dialog tests (`R3-B1`): real `req/` path,
//! state-dir `report show` prefill on both local and remote studios,
//! `v0 → v1` empty-editor fast path, loading guard, and two-step
//! overwrite confirmation when the base requirement read failed.

use super::*;
use crate::studio::StudioKey;
use crate::studio_dialog::StudioDialog;
use crate::studio_menu::{StudioAct, StudioMenu};

type Recorded =
    std::sync::Arc<std::sync::Mutex<Vec<(Option<String>, Vec<String>, Option<String>)>>>;

fn recorder(sink: Recorded, fetch_reply: Result<String, String>) -> crate::app::new_studio::NsRun {
    std::sync::Arc::new(move |machine, argv, stdin| {
        sink.lock().unwrap().push((machine, argv.clone(), stdin));
        let out = if argv.first().map(String::as_str) == Some("report") {
            fetch_reply.clone()
        } else {
            Ok(String::new())
        };
        Box::pin(async move { out })
    })
}

async fn drain_dialog_fetch(app: &mut App) {
    for _ in 0..20 {
        tokio::task::yield_now().await;
        while let Ok(reply) = app.dialog_fetch_rx.try_recv() {
            app.on_dialog_fetch(reply);
        }
    }
}

fn ctrl_s(app: &mut App) {
    app.on_input(
        UiEvent::Key(term_key(
            hc_vt::Key::Char('s'),
            hc_vt::Mods::CTRL,
            hc_vt::Action::Press,
        )),
        app.area.0,
    );
}

fn type_summary(app: &mut App, text: &str) {
    key(app, hc_vt::Key::Tab);
    for ch in text.chars() {
        key(app, hc_vt::Key::Char(ch));
    }
}

#[tokio::test]
async fn local_revise_reads_req_path_via_report_show_not_repo() {
    let decoy_repo = crate::test_support::TestDir::new("hctui-revise-decoy");
    std::fs::create_dir_all(decoy_repo.join("requirements")).unwrap();
    std::fs::write(decoy_repo.join("requirements/v1.md"), "DECOY FROM REPO\n").unwrap();

    let mut app = app_with_panel_doc();
    let mut doc = summary_json();
    doc["studios"][0]["repo"] = serde_json::json!(decoy_repo.to_string_lossy());
    doc["studios"][0]["req_file"] = serde_json::json!("req/v1.md");
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.area = (120, 24);

    let calls: Recorded = Default::default();
    app.set_ns_run(recorder(
        calls.clone(),
        Ok("base spec from state dir".into()),
    ));
    open_panel(&mut app);

    key(&mut app, hc_vt::Key::Char('/'));
    for ch in "revise".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    assert!(matches!(app.studio_dialog, Some(StudioDialog::Revise(_))));

    drain_dialog_fetch(&mut app).await;
    let Some(StudioDialog::Revise(d)) = &app.studio_dialog else {
        panic!("revise dialog should be open");
    };
    assert_eq!(d.error, None);
    assert!(!d.prefill_failed);
    assert_eq!(
        d.body.as_ref().map(|b| b.text()).as_deref(),
        Some("base spec from state dir")
    );

    {
        let recorded = calls.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, None);
        assert_eq!(
            recorded[0].1,
            vec![
                "report",
                "show",
                "--studio",
                "acme-shop",
                "--file",
                "req/v1.md"
            ]
        );
    }

    type_summary(&mut app, "clarify scope");
    ctrl_s(&mut app);
    assert!(app.studio_dialog.is_none(), "submit closes dialog");

    let recorded = calls.lock().unwrap();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[1].0, None);
    assert_eq!(
        recorded[1].1,
        vec![
            "req",
            "revise",
            "--stdin",
            "--summary",
            "clarify scope",
            "--studio",
            "acme-shop",
            "--as",
            "user",
        ]
    );
    assert_eq!(recorded[1].2.as_deref(), Some("base spec from state dir"));
}

#[tokio::test]
async fn remote_revise_uses_summary_req_file_and_submits_remotely() {
    let mut app = app_with_two_machines();
    let mut rdoc = remote_summary_json();
    rdoc["studios"][0]["req_file"] = serde_json::json!("req/v1-checkout.md");
    app.view.documents_mut().insert(
        DocKey {
            machine: Some("host-b".into()),
            name: "backlot".into(),
        },
        Document {
            value: rdoc,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.area = (120, 24);

    let calls: Recorded = Default::default();
    app.set_ns_run(recorder(calls.clone(), Ok("remote spec".into())));

    let machines = app.panel_summaries();
    let beta_key = StudioKey {
        machine: Some("host-b".into()),
        id: "beta-tools".into(),
    };
    let beta = crate::panel::data::studio(&machines, &beta_key).expect("beta-tools");
    let menu = StudioMenu::studio(&beta.key(), beta.studio, false, false);
    app.run_studio_act(&menu, StudioAct::Revise);

    drain_dialog_fetch(&mut app).await;
    {
        let recorded = calls.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0.as_deref(), Some("host-b"));
        assert_eq!(
            recorded[0].1,
            vec![
                "report",
                "show",
                "--studio",
                "beta-tools",
                "--file",
                "req/v1-checkout.md",
            ]
        );
    }

    type_summary(&mut app, "update api");
    ctrl_s(&mut app);
    assert!(app.studio_dialog.is_none());

    let recorded = calls.lock().unwrap();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[1].0.as_deref(), Some("host-b"));
    assert_eq!(
        recorded[1].1,
        vec![
            "req",
            "revise",
            "--stdin",
            "--summary",
            "update api",
            "--studio",
            "beta-tools",
            "--as",
            "user",
        ]
    );
    assert_eq!(recorded[1].2.as_deref(), Some("remote spec"));
}

#[tokio::test]
async fn from_zero_opens_empty_editor_with_no_fetch_and_submits_v1() {
    let mut app = app_with_panel_doc();
    let mut doc = summary_json();
    doc["studios"][0]["requirement_version"] = serde_json::json!(0);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.area = (120, 24);

    let calls: Recorded = Default::default();
    app.set_ns_run(recorder(calls.clone(), Err("should never fetch v0".into())));

    let machines = app.panel_summaries();
    let s = crate::panel::data::studio(&machines, &StudioKey::local("acme-shop")).unwrap();
    let menu = StudioMenu::studio(&s.key(), s.studio, false, false);
    app.run_studio_act(&menu, StudioAct::Revise);

    let Some(StudioDialog::Revise(d)) = &app.studio_dialog else {
        panic!("revise dialog should be open");
    };
    assert!(d.title.contains("v0 → v1"), "{}", d.title);
    assert_eq!(d.error, None);
    assert!(!d.prefill_failed);
    assert_eq!(d.body.as_ref().map(|b| b.text()).as_deref(), Some(""));
    tokio::task::yield_now().await;
    assert!(calls.lock().unwrap().is_empty(), "no fetch for v0");

    for ch in "first spec".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    type_summary(&mut app, "initial spec");
    ctrl_s(&mut app);
    assert!(app.studio_dialog.is_none());

    let recorded = calls.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].1[0..2], ["req", "revise"]);
    assert_eq!(recorded[0].2.as_deref(), Some("first spec"));
}

#[tokio::test]
async fn failed_base_read_guards_first_submit_and_confirms_on_second() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    let calls: Recorded = Default::default();
    app.set_ns_run(recorder(
        calls.clone(),
        Err("cannot read req/v1.md: No such file".into()),
    ));
    open_panel(&mut app);

    key(&mut app, hc_vt::Key::Char('/'));
    for ch in "revise".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);

    // While loading (`body.is_none()`), Ctrl+S blocks with inline message
    // and does not invoke `run_action` (which calls `ns_runner` eagerly).
    ctrl_s(&mut app);
    let Some(StudioDialog::Revise(d)) = &app.studio_dialog else {
        panic!("dialog stays open while loading");
    };
    assert!(
        d.error.as_deref().is_some_and(|e| e.contains("loading")),
        "{:?}",
        d.error
    );
    assert_eq!(calls.lock().unwrap().len(), 0, "no action dispatched");

    // Failed fetch lands.
    drain_dialog_fetch(&mut app).await;
    assert_eq!(calls.lock().unwrap().len(), 1, "only the fetch ran");
    let Some(StudioDialog::Revise(d)) = &app.studio_dialog else {
        panic!("dialog stays open");
    };
    assert!(d.prefill_failed);
    assert!(!d.confirm_overwrite);
    assert_eq!(
        d.error.as_deref(),
        Some("cannot read req/v1.md: No such file")
    );

    // First submit with a summary arms overwrite confirmation, 0 revises dispatched.
    type_summary(&mut app, "replacement spec");
    ctrl_s(&mut app);
    let Some(StudioDialog::Revise(d)) = &app.studio_dialog else {
        panic!("first submit must keep dialog open on failed prefill");
    };
    assert!(d.confirm_overwrite);
    assert!(
        d.error
            .as_deref()
            .is_some_and(|e| e.contains("again to overwrite")),
        "{:?}",
        d.error
    );
    assert_eq!(
        calls.lock().unwrap().len(),
        1,
        "no req revise dispatched on first submit"
    );

    // Second explicit submit confirms and dispatches `req revise`.
    ctrl_s(&mut app);
    assert!(app.studio_dialog.is_none(), "second submit closes dialog");
    let recorded = calls.lock().unwrap();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[1].1[0..2], ["req", "revise"]);
}

#[tokio::test]
async fn send_button_click_follows_same_loading_and_overwrite_guard() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    let calls: Recorded = Default::default();
    app.set_ns_run(recorder(
        calls.clone(),
        Err("cannot read req/v1.md: No such file".into()),
    ));
    open_panel(&mut app);

    key(&mut app, hc_vt::Key::Char('/'));
    for ch in "revise".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    draw_to(&mut app, 120, 24);
    let send = app.studio_dialog_hits.send;
    assert!(!send.is_empty());

    // Clicking Send while loading blocks without dispatching.
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        send.x + 1,
        send.y,
    ));
    assert!(
        matches!(app.studio_dialog, Some(StudioDialog::Revise(ref d)) if d.error.as_deref().is_some_and(|e| e.contains("loading")))
    );
    assert_eq!(calls.lock().unwrap().len(), 0);

    drain_dialog_fetch(&mut app).await;
    type_summary(&mut app, "send button overwrite");
    // First Send click arms confirmation.
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        send.x + 1,
        send.y,
    ));
    let Some(StudioDialog::Revise(d)) = &app.studio_dialog else {
        panic!("first click must arm overwrite confirmation");
    };
    assert!(d.confirm_overwrite);
    assert_eq!(calls.lock().unwrap().len(), 1);

    // Second Send click dispatches `req revise`.
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        send.x + 1,
        send.y,
    ));
    assert!(app.studio_dialog.is_none());
    let recorded = calls.lock().unwrap();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[1].1[0..2], ["req", "revise"]);
}

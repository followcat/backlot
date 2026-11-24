use super::*;
use crate::app::dialogs::Target;
use crate::input::KeybindAction as A;
use crate::structure::{Adoption, Document};
use crate::testcore;
use hc_vt::{Action, Key, Mods};
use serde_json::json;
use std::collections::BTreeMap;

fn pending(app: &mut App) {
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
}

fn empty_online_app() -> App {
    let mut app = app_with_terms(vec![]);
    pending(&mut app);
    app.focused = None;
    app.refresh_projection();
    app
}

fn operations(app: &mut App) -> mpsc::Receiver<testcore::FakeCore> {
    let (accepted, rx) = mpsc::channel(4);
    let (rpc, _sub) = testcore::openers(accepted);
    app.set_operations(rpc);
    app.view.core.as_mut().unwrap().id = "core-fake-1".into();
    rx
}

async fn settle(app: &mut App) {
    let result = tokio::time::timeout(
        testcore::DEADLINE,
        App::operation_result(&mut app.operations),
    )
    .await
    .unwrap();
    app.on_operation(result);
}

fn press(key: Key) -> input::TerminalKey {
    term_key(key, Mods::NONE, Action::Press)
}

async fn serve_create(
    mut rx: mpsc::Receiver<testcore::FakeCore>,
    spawned_address: &str,
    spawned_cwd: &str,
) -> (serde_json::Value, Document, String, String) {
    let mut core = rx.recv().await.unwrap();
    core.read_hello().await;
    core.answer_hello(true, &["doc.get", "doc.set", "terminal.run"], &[])
        .await;
    for _ in 0..2 {
        let read = core.read_request().await;
        assert_eq!(read["method"], "doc.get");
        core.reply(&read["id"], json!({"value": null, "version": 0}))
            .await;
    }
    let run = core.read_request().await;
    assert_eq!(run["method"], "terminal.run");
    let raw_space = run["params"]["labels"]["herdr.space"]
        .as_str()
        .unwrap()
        .to_owned();
    let raw_tab = run["params"]["labels"]["herdr.tab"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut terminal = testcore::minimal_terminal();
    terminal["address"] = json!(spawned_address);
    terminal["cwd"] = json!(spawned_cwd);
    terminal["labels"] = run["params"]["labels"].clone();
    core.reply(&run["id"], terminal).await;
    let read = core.read_request().await;
    assert_eq!(read["method"], "doc.get");
    core.reply(&read["id"], json!({"value": null, "version": 0}))
        .await;
    let write = core.read_request().await;
    assert_eq!(write["method"], "doc.set");
    let document = Document::decode(write["params"]["value"].clone()).unwrap();
    core.reply(&write["id"], json!({"version": 1})).await;
    (run, document, raw_space, raw_tab)
}

#[tokio::test]
async fn new_workspace_on_empty_local_core_omits_cwd_and_adopts_spawned_cwd() {
    let mut app = empty_online_app();
    let rx = operations(&mut app);
    let server = tokio::spawn(serve_create(rx, "t1", "/remote/home/dev"));

    app.on_action(A::NewWorkspace, 80);
    assert_eq!(app.pending_actions, 1);
    settle(&mut app).await;
    assert_eq!(app.pending_actions, 0);
    assert!(app.notice.is_none());

    let (run, document, raw_space, raw_tab) = server.await.unwrap();
    assert!(
        run["params"].get("cwd").is_none(),
        "unexpected cwd: {}",
        run["params"]
    );
    assert!(
        run["params"].get("on").is_none(),
        "unexpected on: {}",
        run["params"]
    );
    let space_key = format!("0:/space:{}:{raw_space}", raw_space.len());
    let adoption = &document.adoptions[&space_key];
    assert_eq!(adoption.name, "dev");
    assert!(!adoption.custom_label);
    assert_eq!(document.focus.as_deref(), Some("t1"));

    let mut spawned = crate::sidebar::fixture::term("t1");
    spawned.cwd = "/remote/home/dev".into();
    spawned.labels.insert("herdr.space".into(), raw_space);
    spawned.labels.insert("herdr.tab".into(), raw_tab);
    spawned.labels.insert("herdr.seen".into(), "0".into());
    let seq = app.view.inventory.seq() + 1;
    app.view.inventory_mut().replace(seq, vec![spawned]);
    app.reconcile_published_view();

    assert_eq!(app.focused.as_deref(), Some("t1"));
    assert_eq!(app.projection.snapshot.workspaces.len(), 1);
    assert_eq!(app.projection.snapshot.workspaces[0].label, "dev");
    assert!(!app.projection.snapshot.workspaces[0].custom_label);
}

#[tokio::test]
async fn new_workspace_on_empty_remote_endpoint_targets_that_machine_without_local_cwd() {
    let mut app = empty_online_app();
    app.focused = None;
    app.projection.active_endpoint_id = ClientEndpointId::Remote("host-a".into());

    let (creation, suggested) = app.workspace_creation().unwrap();
    assert_eq!(creation.params.on.as_deref(), Some("host-a"));
    assert_eq!(creation.params.cwd, None);
    assert_eq!(suggested, "workspace");
    let adoption = creation.space_adoption.as_ref().unwrap();
    assert_eq!(adoption.machine.as_deref(), Some("host-a"));
    assert!(adoption.name.is_empty());
    assert!(!adoption.custom_label);
}

#[tokio::test]
async fn new_workspace_prompt_on_empty_core_keeps_custom_name_or_defers_default() {
    // Submitting a custom name locks custom_label=true and keeps "alpha".
    let mut app = empty_online_app();
    app.shell.prompt_new_workspace_name = true;
    let rx = operations(&mut app);
    let server = tokio::spawn(serve_create(rx, "t1", "/remote/home/dev"));

    app.on_action(A::NewWorkspace, 80);
    app.insert_dialog_text("alpha");
    app.dialog_key(&press(Key::Enter));
    settle(&mut app).await;
    assert!(app.notice.is_none());

    let (_, document, raw_space, _) = server.await.unwrap();
    let space_key = format!("0:/space:{}:{raw_space}", raw_space.len());
    let adoption = &document.adoptions[&space_key];
    assert!(adoption.custom_label);
    assert_eq!(adoption.name, "alpha");

    // Accepting the prefilled "workspace" leaves adoption.name empty until
    // terminal.run replies with the spawned terminal's cwd.
    let mut app = empty_online_app();
    app.shell.prompt_new_workspace_name = true;
    let rx = operations(&mut app);
    let server = tokio::spawn(serve_create(rx, "t1", "/remote/home/dev"));

    app.on_action(A::NewWorkspace, 80);
    let dialog = app.dialog.as_ref().unwrap();
    let Target::NewWorkspace(creation, suggested) = &dialog.target else {
        panic!("expected NewWorkspace dialog");
    };
    assert_eq!(suggested, "workspace");
    assert_eq!(creation.params.cwd, None);
    let before_submit = creation.space_adoption.as_ref().unwrap();
    assert!(before_submit.name.is_empty());
    assert!(!before_submit.custom_label);

    app.dialog_key(&press(Key::Enter));
    settle(&mut app).await;
    assert!(app.notice.is_none());

    let (_, document, raw_space, _) = server.await.unwrap();
    let space_key = format!("0:/space:{}:{raw_space}", raw_space.len());
    let adoption = &document.adoptions[&space_key];
    assert!(!adoption.custom_label);
    assert_eq!(adoption.name, "dev");
}

#[tokio::test]
async fn empty_spawned_cwd_falls_back_to_workspace_and_blank_adoption_never_blanks_label() {
    assert_eq!(super::super::creation::suggested_name(""), "workspace");
    assert_eq!(super::super::creation::suggested_name("/"), "workspace");

    let mut app = empty_online_app();
    let rx = operations(&mut app);
    let server = tokio::spawn(serve_create(rx, "t1", ""));

    app.on_action(A::NewWorkspace, 80);
    settle(&mut app).await;
    assert!(app.notice.is_none());

    let (_, document, raw_space, _) = server.await.unwrap();
    let space_key = format!("0:/space:{}:{raw_space}", raw_space.len());
    assert_eq!(document.adoptions[&space_key].name, "workspace");
    assert!(!document.adoptions[&space_key].custom_label);

    let mut term = crate::sidebar::fixture::term("t1");
    term.cwd = "/home/user/proj".into();
    let own = Document {
        adoptions: BTreeMap::from([(
            "0:/cwd:15:/home/user/proj".into(),
            Adoption {
                space: "ignored".into(),
                name: String::new(),
                ..Default::default()
            },
        )]),
        ..Default::default()
    };
    let mut app = app_with_terms(vec![term]);
    app.view.documents_mut().insert(
        hc_proto::watch::DocKey {
            machine: None,
            name: "hc-tui".into(),
        },
        Arc::new(hc_proto::watch::Document {
            value: serde_json::to_value(&own).unwrap(),
            version: 1,
            ..Default::default()
        }),
    );
    app.refresh_projection();
    assert_eq!(app.projection.snapshot.workspaces[0].label, "proj");
}

#[tokio::test]
async fn stale_focus_or_unprojected_workspace_falls_back_cleanly() {
    let mut app = empty_online_app();
    app.focused = Some("gone".into());
    let (creation, suggested) = app.workspace_creation().unwrap();
    assert_eq!(creation.params.cwd, None);
    assert_eq!(suggested, "workspace");

    let mut term = crate::sidebar::fixture::term("t1");
    term.cwd = "/home/user/proj".into();
    let mut app = app_with_terms(vec![term]);
    app.focused = Some("t1".into());
    app.projection.snapshot.focused_workspace_id = None;
    let (creation, suggested) = app.workspace_creation().unwrap();
    assert_eq!(creation.params.cwd.as_deref(), Some("/home/user/proj"));
    assert_eq!(suggested, "proj");
}

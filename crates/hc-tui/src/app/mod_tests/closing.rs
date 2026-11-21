//! A11: the client's own host is never a "which pane next" candidate —
//! the core refuses a terminal attaching to itself, so an automatic
//! pick would land on the `attach failed: refused` card instead of a
//! sibling or the empty state. Covers the close action's `next`, the
//! publish-time focus eviction and the workspace-switch pane pick.

use super::*;
use crate::chrome::navigate::WorkspaceNavigationTarget;
use crate::operations::{Action, Outcome};
use crate::testcore;
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// Attach machinery that never resolves, focused on `pane`.
fn pending_on(app: &mut App, pane: &str) {
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    app.focused = Some(pane.into());
    app.refresh_projection();
    app.sync_target();
}

fn workspace_id(app: &App, label: &str) -> String {
    app.projection
        .snapshot
        .workspaces
        .iter()
        .find(|w| w.label == label)
        .expect("workspace")
        .workspace_id
        .clone()
}

fn tab_id(app: &App, workspace_id: &str, label: &str) -> String {
    app.projection
        .snapshot
        .tabs
        .iter()
        .find(|t| t.workspace_id == workspace_id && t.label == label)
        .expect("tab")
        .tab_id
        .clone()
}

/// The `next` the pending confirm dialog captured (`confirm_close` is
/// on in the fixture's default config).
fn confirmed_next(app: &App) -> Option<String> {
    let super::super::dialogs::Target::Close(action) = &app.dialog.as_ref().unwrap().target else {
        panic!("the close did not open a confirm dialog")
    };
    let Action::Close { next, .. } = action else {
        panic!("not a close action")
    };
    next.clone()
}

fn target(workspace_id: &str) -> WorkspaceNavigationTarget {
    WorkspaceNavigationTarget {
        endpoint_id: ClientEndpointId::Local,
        workspace_id: workspace_id.into(),
        boot_id: String::new(),
        generation: None,
    }
}

/// A terminal record `terminal.get` answers for a close target — the
/// space/tab labels reproduce the membership ids the projection saw.
fn record(address: &str, created_ms: u64, space: &str, tab: &str) -> Value {
    let mut record = testcore::minimal_terminal();
    record["address"] = json!(address);
    record["created_ms"] = json!(created_ms);
    record["labels"] = json!({"herdr.space": space, "herdr.tab": tab});
    record
}

/// Serves the framed conn a `Close` action opens: the capability
/// preflight read, then per terminal — get (a record on its real
/// space/tab), the foreign membership read, kill, and the persisting
/// doc get/set pair.
async fn serve_close(mut core: testcore::FakeCore, closed: Vec<Value>) {
    core.read_hello().await;
    core.answer_hello(
        true,
        &["doc.get", "doc.set", "terminal.get", "terminal.kill"],
        &[],
    )
    .await;
    let preflight = core.read_request().await;
    assert_eq!(preflight["method"], "doc.get");
    core.reply(&preflight["id"], json!({"value": null, "version": 0}))
        .await;
    let mut written = Value::Null;
    let mut version = 0u64;
    for terminal in closed {
        let get = core.read_request().await;
        assert_eq!(get["method"], "terminal.get");
        core.reply(&get["id"], terminal.clone()).await;
        let foreign = core.read_request().await;
        assert_eq!(foreign["method"], "doc.get");
        assert_eq!(foreign["params"]["name"], "herdr-app");
        core.reply(&foreign["id"], json!({"value": null, "version": 0}))
            .await;
        let kill = core.read_request().await;
        assert_eq!(kill["method"], "terminal.kill");
        core.reply(&kill["id"], terminal).await;
        let read = core.read_request().await;
        assert_eq!(read["method"], "doc.get");
        core.reply(&read["id"], json!({"value": written, "version": version}))
            .await;
        let write = core.read_request().await;
        assert_eq!(write["method"], "doc.set");
        written = write["params"]["value"].clone();
        version += 1;
        core.reply(&write["id"], json!({"version": version})).await;
    }
}

/// The store's next result, unwrapped of the timeout envelope.
async fn settle(app: &mut App) -> Outcome {
    tokio::time::timeout(
        testcore::DEADLINE,
        App::operation_result(&mut app.operations),
    )
    .await
    .expect("action never resolved")
    .expect("action worker stopped")
    .expect("action failed")
}

/// The A11 repro: closing the studio's workspace when the only other
/// pane is the client's own host must leave `next` empty — never the
/// own pane id the core would refuse to attach.
#[tokio::test]
async fn a_workspace_close_never_falls_back_to_the_clients_own_host() {
    let mut app = app_with_tabs(&[("t2", "home", "tab1"), ("t5", "studio", "tab5")]);
    pending_on(&mut app, "t5");
    app.set_own_pane_id(Some("t2".into()));
    let studio = workspace_id(&app, "studio");
    app.close_workspace(&studio);
    assert_eq!(confirmed_next(&app), None);
}

/// `own_pane_id == None` — a remote endpoint or a foreign session —
/// keeps today's pick: the first surviving pane.
#[tokio::test]
async fn a_close_fallback_with_no_own_host_picks_the_first_survivor() {
    let mut app = app_with_tabs(&[("t2", "home", "tab1"), ("t5", "studio", "tab5")]);
    pending_on(&mut app, "t5");
    let studio = workspace_id(&app, "studio");
    app.close_workspace(&studio);
    assert_eq!(confirmed_next(&app).as_deref(), Some("t2"));
}

/// The own filter only skips the host pane: a real sibling still wins
/// the fallback.
#[tokio::test]
async fn a_close_fallback_skips_own_for_a_surviving_sibling() {
    let mut app = app_with_tabs(&[
        ("t2", "home", "tab1"),
        ("t5", "studio", "tab5"),
        ("t9", "away", "tab9"),
    ]);
    pending_on(&mut app, "t5");
    app.set_own_pane_id(Some("t2".into()));
    let studio = workspace_id(&app, "studio");
    app.close_workspace(&studio);
    assert_eq!(confirmed_next(&app).as_deref(), Some("t9"));
}

/// Closing a non-last tab looks at the remaining tab's layout focus and
/// same-tab panes before the global scan — all three skip own, so the
/// only remaining candidate being the host leaves `next` empty.
#[tokio::test]
async fn a_last_tab_close_writes_no_focus_when_only_own_remains() {
    let (accepted, mut rpc_rx) = mpsc::channel(4);
    let (rpc, _sub) = testcore::openers(accepted);
    let mut app = app_with_tabs(&[("t2", "mix", "a"), ("t5", "mix", "b")]);
    pending_on(&mut app, "t5");
    app.set_own_pane_id(Some("t2".into()));
    app.view.core.as_mut().unwrap().id = "core-fake-1".into();
    app.set_operations(rpc);
    let mix = workspace_id(&app, "mix");
    let b = tab_id(&app, &mix, "b");
    let created_ms = app.view.inventory.get("t5").unwrap().created_ms;
    let server = tokio::spawn(async move {
        serve_close(
            rpc_rx.recv().await.unwrap(),
            vec![record("t5", created_ms, "mix", "b")],
        )
        .await;
    });
    app.close_tab_for(&mix, &b);
    let Outcome::Written(written) = settle(&mut app).await else {
        panic!("a close resolves to a written document")
    };
    assert_eq!(written.document.focus, None);
    server.await.unwrap();
}

/// The same-tab survivors the pane-close path prefers drop own too:
/// the sibling pane wins the written focus.
#[tokio::test]
async fn a_pane_close_writes_the_surviving_sibling_not_own() {
    let (accepted, mut rpc_rx) = mpsc::channel(4);
    let (rpc, _sub) = testcore::openers(accepted);
    let mut app = app_with_tabs(&[("t2", "mix", "a"), ("t5", "mix", "a"), ("t6", "mix", "a")]);
    pending_on(&mut app, "t5");
    app.set_own_pane_id(Some("t2".into()));
    app.view.core.as_mut().unwrap().id = "core-fake-1".into();
    app.set_operations(rpc);
    let mix = workspace_id(&app, "mix");
    let a = tab_id(&app, &mix, "a");
    let created_ms = app.view.inventory.get("t5").unwrap().created_ms;
    let server = tokio::spawn(async move {
        serve_close(
            rpc_rx.recv().await.unwrap(),
            vec![record("t5", created_ms, "mix", "a")],
        )
        .await;
    });
    app.close_pane_for(&mix, &a, "t5");
    let Outcome::Written(written) = settle(&mut app).await else {
        panic!("a close resolves to a written document")
    };
    assert_eq!(written.document.focus.as_deref(), Some("t6"));
    server.await.unwrap();
}

/// The publish that evicts the focused pane picks a replacement the
/// same way: own is skipped, and with own reported the focus stays
/// empty instead of attaching to the refused host.
#[tokio::test]
async fn a_lost_focus_never_falls_back_to_the_clients_own_host() {
    for own in [None, Some("t2")] {
        let mut app = app_with_tabs(&[("t2", "home", "tab1"), ("t5", "studio", "tab5")]);
        pending_on(&mut app, "t5");
        app.set_own_pane_id(own.map(str::to_owned));
        let t2 = app.view.inventory.get("t2").unwrap().clone();
        app.view.inventory_mut().replace(2, vec![t2]);
        app.reconcile_published_view();
        let want = if own.is_some() { None } else { Some("t2") };
        assert_eq!(app.focused.as_deref(), want, "own={own:?}");
    }
}

/// A workspace switch targets the active tab's first non-own pane —
/// and stays without a target when the workspace holds only the host.
#[tokio::test]
async fn a_workspace_switch_never_picks_the_clients_own_host() {
    let mut app = app_with_tabs(&[
        ("t2", "host", "a"),
        ("t6", "host", "a"),
        ("t9", "solo", "c"),
    ]);
    pending_on(&mut app, "t9");
    app.set_own_pane_id(Some("t2".into()));
    let host = workspace_id(&app, "host");
    assert_eq!(
        app.workspace_terminal(&target(&host)).as_deref(),
        Some("t6")
    );

    let mut app = app_with_tabs(&[("t2", "lone", "d"), ("t9", "solo", "c")]);
    pending_on(&mut app, "t9");
    app.set_own_pane_id(Some("t2".into()));
    let lone = workspace_id(&app, "lone");
    assert_eq!(app.workspace_terminal(&target(&lone)), None);
}

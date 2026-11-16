use super::floats::*;
use super::*;
use crate::app::float::{FloatEntry, FloatKey};
use std::time::Duration;

#[tokio::test]
async fn an_owner_tab_close_kills_its_parked_float() {
    let mut app = two_spaces();
    let key = spawn_shown(&mut app, "t9", 9);
    let mut terms = base_terms();
    terms.push(float_term("t9", &key.owner, 9));
    publish(&mut app, terms);
    app.focused = Some("t2".into());
    app.reconcile_published_view();
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.get", "terminal.kill"], &[])
            .await;
        let get = core.read_request().await;
        assert_eq!(get["method"], "terminal.get");
        assert_eq!(get["params"]["terminal"], "t9");
        core.reply(&get["id"], run_reply("t9", 9)).await;
        let kill = core.read_request().await;
        assert_eq!(kill["method"], "terminal.kill");
        assert_eq!(kill["params"]["terminal"], "t9");
        core.reply(&kill["id"], run_reply("t9", 9)).await;
    });
    // t1's workspace closes — its tab disappears from the publish.
    let mut t2 = crate::sidebar::fixture::term("t2");
    t2.created_ms = 1;
    t2.labels.insert("herdr.space".into(), "space-t2".into());
    t2.labels.insert("herdr.tab".into(), "tab".into());
    let t9 = float_term("t9", &key.owner, 9);
    publish(&mut app, vec![t2.clone(), t9]);
    settle(&mut app).await;
    server.await.unwrap();
    // The kill is in flight; the entry leaves once the record does —
    // rediscovery must not resurrect it meanwhile.
    publish(&mut app, vec![t2]);
    assert!(app.floats.is_empty());
    assert!(!app.size_leases.contains_key("t9"));
}

#[tokio::test]
async fn a_labeled_float_is_adopted_without_a_resize() {
    let mut app = two_spaces();
    let owner = app.focused_owner_key().unwrap();
    let mut terms = base_terms();
    let mut t9 = float_term("t9", &owner, 9);
    t9.cols = 132;
    t9.rows = 43;
    terms.push(t9);
    publish(&mut app, terms);
    let key = FloatKey {
        owner,
        command: "alt+f".into(),
    };
    let entry = app.floats.get(&key).expect("the record is adopted");
    assert_eq!(entry.terminal, "t9");
    assert!(!entry.restore, "rediscovered floats stay hidden");
    assert_eq!(entry.created_ms, 9);
    assert!(app.size_leases.contains_key("t9"));
}

#[tokio::test]
async fn an_unlabeled_popup_record_is_left_alone() {
    let mut app = two_spaces();
    let mut t9 = crate::sidebar::fixture::term("t9");
    t9.created_ms = 9;
    t9.labels.insert("hc-tui.popup".into(), "1".into());
    let mut terms = base_terms();
    terms.push(t9);
    publish(&mut app, terms);
    assert!(app.floats.is_empty(), "no owner label, no adoption");
    assert!(!app.size_leases.contains_key("t9"));
}

#[tokio::test]
async fn an_ephemeral_spawn_cannot_steal_a_pending_float_run() {
    let mut app = two_spaces();
    app.run_custom_command(&command("exec \"${SHELL:-sh}\""));
    assert!(
        app.popup_pending
            .as_ref()
            .is_some_and(|pending| pending.persistent)
    );
    app.run_custom_command(&ephemeral("htop"));
    let pending = app
        .popup_pending
        .as_ref()
        .expect("the float run keeps its pending arm");
    assert!(pending.persistent);
    assert_eq!(pending.command, "alt+f");
}

#[tokio::test]
async fn a_host_resize_refits_a_hidden_float_lease() {
    let mut app = two_spaces();
    app.area = (80, 24);
    let key = spawn_shown(&mut app, "t9", 9);
    let mut terms = base_terms();
    terms.push(float_term("t9", &key.owner, 9));
    publish(&mut app, terms);
    app.focused = Some("t2".into());
    app.reconcile_published_view();
    let before = app.size_leases["t9"].allocated_size();
    app.area = (160, 48);
    app.popup_resize();
    let after = app.size_leases["t9"].allocated_size();
    assert_ne!(before, after, "the hidden lease refits on host resize");
    assert!(after.0 > before.0 && after.1 > before.1);
}

/// A labeled terminal — endpoint snapshots need the labels to list
/// their tabs, so publishes carry them too.
fn labeled_tab(address: &str, space: &str, tab: &str) -> hc_proto::terminal::Terminal {
    let mut term = crate::sidebar::fixture::term(address);
    term.pid = Some(1);
    term.labels.insert("herdr.space".into(), space.into());
    term.labels.insert("herdr.tab".into(), tab.into());
    term
}

fn labeled(address: &str, space: &str) -> hc_proto::terminal::Terminal {
    labeled_tab(address, space, "tab")
}

/// A local `t1` plus a remote `host-a/t3`, each the only tab of its own
/// workspace — the remote endpoint's status comes from `machine.state`.
fn remote_spaces(state: &str) -> App {
    let mut app = app();
    let mut view = View {
        link: hc_proto::watch::Link::Online,
        rpc: hc_proto::watch::Link::Online,
        core: Some(hc_proto::hello::CoreInfo {
            id: "id-host-a".into(),
            name: "host-a".into(),
            hostname: None,
        }),
        machines: Some(vec![crate::sidebar::fixture::machine("host-a", state)]),
        ..Default::default()
    };
    view.inventory_mut().replace(
        1,
        vec![labeled("t1", "space-t1"), labeled("host-a/t3", "space-r")],
    );
    app.view = view;
    pending(&mut app);
    app.refresh_projection();
    app
}

/// The qualified `layout_key` owner of host-a's single tab.
fn remote_owner(app: &App) -> String {
    let tab_id = app
        .projection
        .endpoints
        .iter()
        .find(|e| {
            e.endpoint_id == crate::chrome::snapshot::ClientEndpointId::Remote("host-a".into())
        })
        .and_then(|e| e.snapshot.as_ref())
        .and_then(|snapshot| snapshot.tabs.first())
        .map(|tab| tab.tab_id.clone())
        .expect("the remote snapshot lists its tab");
    crate::project::layout_key(Some("host-a"), &tab_id)
}

fn hidden_float_as(app: &mut App, owner: String, terminal: &str, created_ms: u64) -> FloatKey {
    let key = FloatKey {
        owner,
        command: "alt+f".into(),
    };
    app.hide_float(
        key.clone(),
        FloatEntry {
            terminal: terminal.into(),
            width: None,
            height: None,
            restore: false,
            created_ms,
            seen: false,
            kill_sent: false,
        },
    );
    key
}

fn hidden_float(app: &mut App, owner: String) -> FloatKey {
    hidden_float_as(app, owner, "t9", 9)
}

#[tokio::test]
async fn a_remote_owner_tab_close_kills_its_parked_float() {
    let mut app = remote_spaces("connected");
    let owner = remote_owner(&app);
    let key = hidden_float(&mut app, owner.clone());
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.get", "terminal.kill"], &[])
            .await;
        // `terminal.get` fetches the record's `created_ms` — the kill
        // only fires when it matches the entry's incarnation.
        let get = core.read_request().await;
        assert_eq!(get["method"], "terminal.get");
        core.reply(&get["id"], run_reply("t9", 9)).await;
        let kill = core.read_request().await;
        assert_eq!(kill["method"], "terminal.kill");
        assert_eq!(kill["params"]["terminal"], "t9");
        core.reply(&kill["id"], run_reply("t9", 9)).await;
    });
    let t1 = labeled("t1", "space-t1");
    let t3 = labeled("host-a/t3", "space-r");
    let t9 = float_term("t9", &owner, 9);
    publish(&mut app, vec![t1.clone(), t3, t9.clone()]);
    assert!(
        !app.floats[&key].kill_sent,
        "the remote tab is still listed"
    );
    // The remote tab closes: t3 disappears from inventory and its
    // Online snapshot loses the tab — the parked float must die.
    publish(&mut app, vec![t1.clone(), t9]);
    assert!(app.floats[&key].kill_sent, "the owner-gone kill left");
    settle(&mut app).await;
    server.await.unwrap();
    // The record finally leaves inventory — the entry follows.
    publish(&mut app, vec![t1]);
    assert!(app.floats.is_empty());
    assert!(!app.size_leases.contains_key("t9"));
}

#[tokio::test]
async fn an_owner_gone_kill_goes_out_once() {
    let mut app = two_spaces();
    let key = spawn_shown(&mut app, "t9", 9);
    let mut terms = base_terms();
    terms.push(float_term("t9", &key.owner, 9));
    publish(&mut app, terms);
    app.focused = Some("t2".into());
    app.reconcile_published_view();
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.get", "terminal.kill"], &[])
            .await;
        let get = core.read_request().await;
        core.reply(&get["id"], run_reply("t9", 9)).await;
        let kill = core.read_request().await;
        assert_eq!(kill["method"], "terminal.kill");
        core.reply(&kill["id"], run_reply("t9", 9)).await;
        // The record lingers in inventory — reconcile must not repeat
        // the kill while it is still published.
        assert!(
            tokio::time::timeout(Duration::from_millis(200), core.read_request())
                .await
                .is_err(),
            "the owner-gone kill is sent once"
        );
    });
    // t1's workspace closes — its tab disappears from the publish.
    let t2 = base_terms().swap_remove(1);
    publish(&mut app, vec![t2.clone(), float_term("t9", &key.owner, 9)]);
    settle(&mut app).await;
    // Same state published again: still no second kill.
    publish(&mut app, vec![t2.clone(), float_term("t9", &key.owner, 9)]);
    server.await.unwrap();
    // The record finally leaves — the entry follows.
    publish(&mut app, vec![t2]);
    assert!(app.floats.is_empty());
    assert!(!app.size_leases.contains_key("t9"));
}

#[tokio::test]
async fn an_offline_endpoint_keeps_its_float() {
    let mut app = remote_spaces("offline");
    let owner = remote_owner(&app);
    let key = hidden_float(&mut app, owner.clone());
    let t1 = labeled("t1", "space-t1");
    let t3 = labeled("host-a/t3", "space-r");
    publish(&mut app, vec![t1.clone(), t3, float_term("t9", &owner, 9)]);
    // The remote tab's record is gone but the endpoint is not Online —
    // presence is unknown, so owner-gone must never kill.
    publish(&mut app, vec![t1.clone(), float_term("t9", &owner, 9)]);
    let entry = app.floats.get(&key).expect("the float survives offline");
    assert_eq!(entry.terminal, "t9");
    assert!(!entry.kill_sent);
}

#[tokio::test]
async fn a_workspace_close_kills_every_float_its_tabs_parked() {
    // One workspace, two tabs — each parked its own float. Closing the
    // workspace drops both tabs in one publish, so both owner-gone
    // kills go out in map order (tab-1's owner sorts first).
    let mut app = app_with_tabs(&[
        ("t1", "space-a", "tab-1"),
        ("t2", "space-a", "tab-2"),
        ("t3", "space-b", "tab"),
    ]);
    pending(&mut app);
    app.reconcile_published_view();
    let owner1 = crate::project::layout_key(None, "tab-1");
    let owner2 = crate::project::layout_key(None, "tab-2");
    let key1 = hidden_float_as(&mut app, owner1.clone(), "t9", 9);
    let key2 = hidden_float_as(&mut app, owner2.clone(), "t8", 8);
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.get", "terminal.kill"], &[])
            .await;
        for (terminal, created_ms) in [("t9", 9u64), ("t8", 8)] {
            let get = core.read_request().await;
            assert_eq!(get["method"], "terminal.get");
            assert_eq!(get["params"]["terminal"], terminal);
            core.reply(&get["id"], run_reply(terminal, created_ms))
                .await;
            let kill = core.read_request().await;
            assert_eq!(kill["method"], "terminal.kill");
            assert_eq!(kill["params"]["terminal"], terminal);
            core.reply(&kill["id"], run_reply(terminal, created_ms))
                .await;
        }
    });
    // Both floats are sighted in inventory first.
    publish(
        &mut app,
        vec![
            labeled_tab("t1", "space-a", "tab-1"),
            labeled_tab("t2", "space-a", "tab-2"),
            labeled("t3", "space-b"),
            float_term("t9", &owner1, 9),
            float_term("t8", &owner2, 8),
        ],
    );
    // space-a closes: the Online snapshot loses both of its tabs.
    publish(
        &mut app,
        vec![
            labeled("t3", "space-b"),
            float_term("t9", &owner1, 9),
            float_term("t8", &owner2, 8),
        ],
    );
    assert!(
        app.floats[&key1].kill_sent && app.floats[&key2].kill_sent,
        "every parked float of the closed workspace gets its kill"
    );
    settle(&mut app).await;
    settle(&mut app).await;
    server.await.unwrap();
    // The records finally leave — both entries and leases follow.
    publish(&mut app, vec![labeled("t3", "space-b")]);
    assert!(app.floats.is_empty());
    assert!(!app.size_leases.contains_key("t9"));
    assert!(!app.size_leases.contains_key("t8"));
}

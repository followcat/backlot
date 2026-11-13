//! Per-tab persistent floats: spawn params, hide/show toggling, tab
//! round-trips, program exit, owner-gone kill, rediscovery, and the
//! request-time owner race.
use super::*;
use crate::app::float::FloatKey;
use crate::config::{CustomCommandAction, CustomCommandKeybind};
use crate::popup_size::PopupSize;
use crate::testcore;
use serde_json::json;
use std::time::Duration;

fn command(line: &str) -> CustomCommandKeybind {
    CustomCommandKeybind {
        bindings: Default::default(),
        label: "alt+f".into(),
        command: line.into(),
        action: CustomCommandAction::Popup,
        description: None,
        width: Some(PopupSize::Percent(80)),
        height: Some(PopupSize::Percent(80)),
        persistent: true,
    }
}

fn pending(app: &mut App) {
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
}

fn operations(app: &mut App) -> mpsc::Receiver<testcore::FakeCore> {
    let (accepted, rx) = mpsc::channel(4);
    let (rpc, _sub) = testcore::openers(accepted);
    app.set_operations(rpc);
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

fn core_id(app: &mut App, id: &str) {
    app.view.core.as_mut().unwrap().id = id.into();
}

fn run_reply(address: &str, created_ms: u64) -> serde_json::Value {
    let mut term = testcore::minimal_terminal();
    term["address"] = json!(address);
    term["created_ms"] = json!(created_ms);
    term
}

/// The float's own terminal record, labeled like a spawned one.
fn float_term(address: &str, owner: &str, created_ms: u64) -> hc_proto::terminal::Terminal {
    let mut term = crate::sidebar::fixture::term(address);
    term.created_ms = created_ms;
    term.labels.insert("hc-tui.popup".into(), "1".into());
    term.labels.insert("hc-tui.owner-tab".into(), owner.into());
    term.labels.insert("hc-tui.command".into(), "alt+f".into());
    term
}

/// Spawn the focused tab's float straight into the shown state — the
/// fake `terminal.run` answer lands without a wire.
fn spawn_shown(app: &mut App, terminal: &str, created_ms: u64) -> FloatKey {
    let key = FloatKey {
        owner: app.focused_owner_key().expect("a focused tab"),
        command: "alt+f".into(),
    };
    app.run_custom_command(&command("exec \"${SHELL:-sh}\""));
    assert!(app.popup_pending.is_some(), "the float run is armed");
    app.on_popup_spawned(terminal.into(), created_ms);
    assert!(app.popup.is_some(), "the float shows");
    key
}

/// Inventory publish carrying every live record, then reconcile.
fn publish(app: &mut App, terms: Vec<hc_proto::terminal::Terminal>) {
    let seq = app.view.inventory.seq() + 1;
    app.view.inventory.replace(seq, terms);
    app.reconcile_published_view();
}

fn base_terms() -> Vec<hc_proto::terminal::Terminal> {
    ["t1", "t2"]
        .iter()
        .enumerate()
        .map(|(i, address)| {
            let mut term = crate::sidebar::fixture::term(address);
            term.created_ms = i as u64;
            term.labels
                .insert("herdr.space".into(), format!("space-{address}"));
            term.labels.insert("herdr.tab".into(), "tab".into());
            term
        })
        .collect()
}

fn two_spaces() -> App {
    let mut app = app_with_spaces(&[("t1", "space-t1"), ("t2", "space-t2")]);
    pending(&mut app);
    app.reconcile_published_view();
    app
}

#[tokio::test]
async fn float_spawn_runs_persistent_with_owner_labels() {
    let mut app = app_with(&["t1"]);
    pending(&mut app);
    app.reconcile_published_view();
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let owner = app.focused_owner_key().unwrap();
    let expected = owner.clone();
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.run"], &[]).await;
        let run = core.read_request().await;
        assert_eq!(run["method"], "terminal.run");
        let params = &run["params"];
        assert_eq!(params["line"], "exec \"${SHELL:-sh}\"");
        assert_eq!(params["keep"], true);
        assert_eq!(params["rm"], true);
        assert_eq!(params["labels"]["hc-tui.popup"], "1");
        assert_eq!(params["labels"]["hc-tui.owner-tab"], expected);
        assert_eq!(params["labels"]["hc-tui.command"], "alt+f");
        assert_eq!(params["cwd"], "/home/user/proj");
        assert!(params.get("on").is_none());
        core.reply(&run["id"], run_reply("t9", 9)).await;
    });
    app.run_custom_command(&command("exec \"${SHELL:-sh}\""));
    settle(&mut app).await;
    server.await.unwrap();
    let popup = app.popup.as_ref().expect("the float shows");
    assert!(popup.persistent);
    assert_eq!(popup.terminal, "t9");
    assert_eq!(popup.key.as_ref().unwrap().owner, owner);
}

#[tokio::test]
async fn float_cwd_prefers_matching_repo_root() {
    let mut app = app_with(&["t1"]);
    app.reconcile_published_view();
    let t1 = app.view.inventory.get("t1").unwrap();
    assert_eq!(app.float_cwd().as_deref(), Some("/home/user/proj"));
    // Foreground cwd wins over the launch cwd as the effective one.
    let mut t1 = t1.clone();
    t1.foreground = Some(hc_proto::terminal::Foreground {
        cwd: "/home/user/proj/sub".into(),
        argv: Vec::new(),
        pid: 100,
        name: "sh".into(),
    });
    t1.repository = Some(hc_proto::terminal::RepositoryFacts {
        core_id: "id-host-a".into(),
        cwd: "/home/user/proj/sub".into(),
        state: "ready".into(),
        auto_label: None,
        checkout: Some(hc_proto::terminal::Checkout {
            repository_key: "repo".into(),
            checkout_key: "co".into(),
            repo_name: "acme-shop".into(),
            checkout_name: "main".into(),
            repo_root: "/home/user/proj".into(),
            is_linked_worktree: false,
            is_bare: false,
        }),
        branch: None,
        ahead_behind: None,
    });
    app.view.inventory.replace(2, vec![t1.clone()]);
    app.reconcile_published_view();
    assert_eq!(app.float_cwd().as_deref(), Some("/home/user/proj"));
    // Facts still describing the old directory cannot claim the new cwd —
    // the workspace folder is the documented fallback.
    let mut stale = t1;
    stale.foreground = Some(hc_proto::terminal::Foreground {
        cwd: "/home/user/elsewhere".into(),
        argv: Vec::new(),
        pid: 100,
        name: "sh".into(),
    });
    app.view.inventory.replace(3, vec![stale]);
    app.reconcile_published_view();
    assert_eq!(app.float_cwd().as_deref(), Some("/home/user/proj"));
}

#[tokio::test]
async fn tab_switch_hides_the_float_but_keeps_its_program() {
    let mut app = two_spaces();
    let key = spawn_shown(&mut app, "t9", 9);
    let mut terms = base_terms();
    terms.push(float_term("t9", &key.owner, 9));
    publish(&mut app, terms.clone());
    app.focused = Some("t2".into());
    app.reconcile_published_view();
    assert!(app.popup.is_none(), "the float hides with its tab");
    let entry = app.floats.get(&key).expect("the float is parked");
    assert_eq!(entry.terminal, "t9");
    assert!(entry.restore, "it was visible when focus left");
    let lease = app.size_leases.get("t9").expect("a hidden size lease");
    assert_eq!(lease.target(), Some("t9"), "the program keeps its attach");
}

#[tokio::test]
async fn switching_back_restores_a_float_that_was_visible() {
    let mut app = two_spaces();
    let key = spawn_shown(&mut app, "t9", 9);
    let mut terms = base_terms();
    terms.push(float_term("t9", &key.owner, 9));
    publish(&mut app, terms);
    app.focused = Some("t2".into());
    app.reconcile_published_view();
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    let popup = app.popup.as_ref().expect("the float comes back");
    assert_eq!(popup.terminal, "t9");
    assert_eq!(popup.key.as_ref(), Some(&key));
    assert!(!app.floats.contains_key(&key));
    assert!(!app.size_leases.contains_key("t9"));
}

#[tokio::test]
async fn an_explicit_hide_stays_hidden_across_a_tab_round_trip() {
    let mut app = two_spaces();
    let key = spawn_shown(&mut app, "t9", 9);
    let mut terms = base_terms();
    terms.push(float_term("t9", &key.owner, 9));
    publish(&mut app, terms);
    app.run_custom_command(&command("exec \"${SHELL:-sh}\""));
    assert!(app.popup.is_none(), "Alt+F hides the float");
    assert!(
        !app.floats[&key].restore,
        "an explicit hide is not restored"
    );
    app.focused = Some("t2".into());
    app.reconcile_published_view();
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    assert!(app.popup.is_none(), "the float stays hidden on return");
    assert!(app.floats.contains_key(&key));
}

#[tokio::test]
async fn float_exit_drops_the_entry_and_the_lease() {
    let mut app = two_spaces();
    let key = spawn_shown(&mut app, "t9", 9);
    let mut terms = base_terms();
    terms.push(float_term("t9", &key.owner, 9));
    publish(&mut app, terms);
    app.focused = Some("t2".into());
    app.reconcile_published_view();
    assert!(app.floats.contains_key(&key));
    // `rm` reaped the finished terminal — the hidden entry follows.
    publish(&mut app, base_terms());
    assert!(app.floats.is_empty());
    assert!(!app.size_leases.contains_key("t9"));
    assert!(app.popup.is_none());
    // The next Alt+F starts a fresh terminal.
    app.run_custom_command(&command("exec \"${SHELL:-sh}\""));
    assert!(app.popup_pending.is_some());
}

#[tokio::test]
async fn a_hidden_float_survives_a_spawn_elsewhere() {
    let mut app = two_spaces();
    let key = spawn_shown(&mut app, "t9", 9);
    let mut terms = base_terms();
    terms.push(float_term("t9", &key.owner, 9));
    publish(&mut app, terms);
    app.focused = Some("t2".into());
    app.reconcile_published_view();
    // Spawning another tab's float only runs; the parked one stays.
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.run"], &[]).await;
        let run = core.read_request().await;
        assert_eq!(run["method"], "terminal.run");
        core.reply(&run["id"], run_reply("t8", 8)).await;
        // No kill may follow a demote — the parked program is untouched.
        assert!(
            tokio::time::timeout(Duration::from_millis(200), core.read_request())
                .await
                .is_err(),
            "no terminal.get/terminal.kill follows"
        );
    });
    app.run_custom_command(&command("exec \"${SHELL:-sh}\""));
    settle(&mut app).await;
    server.await.unwrap();
    let entry = app.floats.get(&key).expect("t1's float still parked");
    assert_eq!(entry.terminal, "t9");
    assert!(app.size_leases.contains_key("t9"));
    // And t2's float is now the visible one.
    assert_eq!(
        app.popup
            .as_ref()
            .and_then(|p| p.key.as_ref())
            .map(|k| k.owner.as_str()),
        Some(app.focused_owner_key().as_deref().unwrap_or_default()),
    );
}

#[tokio::test]
async fn a_run_error_frees_the_pending_float() {
    let mut app = two_spaces();
    app.run_custom_command(&command("exec \"${SHELL:-sh}\""));
    assert!(app.popup_pending.is_some());
    app.on_operation(Some(Err("run failed".into())));
    assert!(app.popup_pending.is_none(), "the pending arm releases");
}

#[tokio::test]
async fn a_late_spawn_parks_under_the_request_time_owner() {
    let mut app = two_spaces();
    let key = FloatKey {
        owner: app.focused_owner_key().unwrap(),
        command: "alt+f".into(),
    };
    app.run_custom_command(&command("exec \"${SHELL:-sh}\""));
    assert!(app.popup_pending.is_some());
    app.focused = Some("t2".into());
    app.reconcile_published_view();
    // `Outcome::Ran` lands after the switch — it registers hidden under
    // the old owner, never on t2's tab.
    app.on_popup_spawned("t9".into(), 9);
    assert!(app.popup.is_none());
    let entry = app.floats.get(&key).expect("parked under its own tab");
    assert_eq!(entry.terminal, "t9");
    assert!(app.size_leases.contains_key("t9"));
}

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
        core.reply(&kill["id"], json!({})).await;
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

//! M4 slice 5: the floating popup terminal, `[[keys.command]]` dispatch
//! and the worker-peek overlay.
use super::*;
use crate::config::{CustomCommandAction, CustomCommandKeybind};
use crate::popup_size::PopupSize;
use crate::testcore;
use hc_vt::{Action, Key, Mods};
use serde_json::json;

fn press(key: Key) -> input::TerminalKey {
    term_key(key, Mods::NONE, Action::Press)
}

fn key(app: &mut App, key: Key) {
    app.on_input(input::UiEvent::Key(press(key)), 120);
}

fn command(action: CustomCommandAction, line: &str) -> CustomCommandKeybind {
    CustomCommandKeybind {
        bindings: Default::default(),
        label: "alt+f".into(),
        command: line.into(),
        action,
        description: None,
        width: Some(PopupSize::Percent(80)),
        height: Some(PopupSize::Percent(80)),
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

fn run_reply() -> serde_json::Value {
    let mut term = testcore::minimal_terminal();
    term["address"] = json!("t9");
    term["created_ms"] = json!(9);
    term
}

fn get_reply(address: &str, created_ms: u64) -> serde_json::Value {
    let mut term = testcore::minimal_terminal();
    term["address"] = json!(address);
    term["created_ms"] = json!(created_ms);
    term
}

/// The core id the operations store guards on must match `view.core`.
fn core_id(app: &mut App, id: &str) {
    app.view.core.as_mut().unwrap().id = id.into();
}

#[tokio::test]
async fn popup_command_runs_terminal_run_and_opens_the_popup() {
    let mut app = app_with(&["t1"]);
    pending(&mut app);
    app.reconcile_published_view();
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.run"], &[]).await;
        let run = core.read_request().await;
        assert_eq!(run["method"], "terminal.run");
        assert_eq!(run["params"]["line"], "echo hi");
        assert_eq!(run["params"]["labels"]["hc-tui.popup"], "1");
        core.reply(&run["id"], run_reply()).await;
    });
    app.run_custom_command(&command(CustomCommandAction::Popup, "echo hi"));
    settle(&mut app).await;
    server.await.unwrap();
    let popup = app.popup.as_ref().expect("Run opens the popup");
    assert_eq!(popup.terminal, "t9");
    assert_eq!(popup.pane.target(), Some("t9"));
}

#[tokio::test]
async fn popup_eats_keys_and_terminal_loss_closes_with_a_kill() {
    let mut app = app_with(&["t1"]);
    pending(&mut app);
    app.reconcile_published_view();
    // t9 exists in the inventory carrying the popup label.
    let mut t9 = crate::sidebar::fixture::term("t9");
    t9.created_ms = 9;
    t9.labels.insert("hc-tui.popup".into(), "1".into());
    app.view.inventory.replace(1, vec![t9]);
    app.reconcile_published_view();
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.get", "terminal.kill"], &[])
            .await;
        let get = core.read_request().await;
        core.reply(&get["id"], get_reply("t9", 9)).await;
        let kill = core.read_request().await;
        assert_eq!(kill["method"], "terminal.kill");
        assert_eq!(kill["params"]["terminal"], "t9");
        core.reply(&kill["id"], json!({})).await;
    });
    app.popup_pending = Some(super::super::popup::PopupPending {
        width: Some(PopupSize::Percent(80)),
        height: Some(PopupSize::Percent(80)),
    });
    app.on_popup_spawned("t9".into());
    // Keys route to the popup attach, never the focused pane — even
    // Escape, which the reference forwards to the popup terminal.
    key(&mut app, Key::Char('x'));
    assert!(app.popup.as_ref().unwrap().pane.input_backlog().0 > 0);
    assert_eq!(app.pane.input_backlog().0, 0);
    key(&mut app, Key::Escape);
    assert!(app.popup.is_some(), "Escape is forwarded, not a close");
    // A tab focus change dismisses the popup; the allow-gone kill cleans
    // up its live `rm` terminal (or tolerates the core already reaping it).
    app.popup.as_mut().unwrap().owner_tab = Some("tab-a".into());
    app.projection.snapshot.focused_tab_id = Some("tab-b".into());
    app.reconcile_published_view();
    assert!(app.popup.is_none(), "tab change dismisses the popup");
    settle(&mut app).await;
    server.await.unwrap();
}

#[tokio::test]
async fn shell_command_dispatches_machine_exec() {
    let mut app = app_with(&["t1"]);
    pending(&mut app);
    app.reconcile_published_view();
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["machine.exec"], &[]).await;
        let exec = core.read_request().await;
        assert_eq!(exec["method"], "machine.exec");
        assert_eq!(exec["params"]["line"], "echo hi");
        core.reply(&exec["id"], json!({"output": "hi\n"})).await;
    });
    app.run_custom_command(&command(CustomCommandAction::Shell, "echo hi"));
    settle(&mut app).await;
    server.await.unwrap();
}

#[tokio::test]
async fn pane_command_runs_zoomed_overlay_and_restores_on_exit() {
    let mut app = app_with_spaces(&[("t1", "work")]);
    pending(&mut app);
    app.reconcile_published_view();
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["doc.get", "doc.set", "terminal.run"], &[])
            .await;
        let mut version = 3u64;
        let mut written = json!({"schema": 1});
        let mut sets = 0;
        while sets < 2 {
            let req = core.read_request().await;
            match req["method"].as_str().unwrap() {
                "doc.get" => {
                    core.reply(&req["id"], json!({"value": written, "version": version}))
                        .await;
                }
                "terminal.run" => {
                    assert_eq!(req["params"]["line"], "echo hi");
                    // A pane command is one-shot: `rm` reaps it, no
                    // shell keeps its place — the reference's command
                    // overlay exits instead of holding the split.
                    assert_eq!(req["params"]["rm"].as_bool(), Some(true));
                    assert_ne!(req["params"]["keep"].as_bool(), Some(true));
                    core.reply(&req["id"], run_reply()).await;
                }
                "doc.set" => {
                    written = req["params"]["value"].clone();
                    version += 1;
                    sets += 1;
                    core.reply(&req["id"], json!({"version": version})).await;
                }
                other => panic!("unexpected request {other}"),
            }
        }
        // The second doc.set is the Forget CAS: the split collapsed back
        // to a single pane, zoom cleared, focus back on the survivor,
        // and the exited terminal tombstoned.
        assert_eq!(written["closed"]["t9"], 9);
        let tab = written["tabs"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap();
        assert_eq!(tab["root"], json!({"type": "pane", "terminal": "t1"}));
        assert_eq!(tab["focused"], "t1");
        assert!(tab["zoomed"].is_null() || tab.get("zoomed").is_none());
    });
    app.run_custom_command(&command(CustomCommandAction::Pane, "echo hi"));
    settle(&mut app).await;
    // The written creation registers the overlay for its terminal.
    let overlay = app.command_overlay.as_ref().expect("pane overlay tracked");
    assert_eq!(overlay.terminal, "t9");
    // The run lands in the inventory, then the program exits and `rm`
    // drops it — the reconcile forgets it from the layout.
    let mut t9 = crate::sidebar::fixture::term("t9");
    t9.created_ms = 9;
    t9.labels.insert("herdr.space".into(), "work".into());
    t9.labels.insert("herdr.tab".into(), "tab".into());
    let mut t1 = crate::sidebar::fixture::term("t1");
    t1.labels.insert("herdr.space".into(), "work".into());
    t1.labels.insert("herdr.tab".into(), "tab".into());
    app.view.inventory.replace(2, vec![t1.clone(), t9]);
    app.reconcile_published_view();
    assert!(app.command_overlay.is_some(), "live overlay terminal keeps");
    app.view.inventory.replace(3, vec![t1]);
    app.reconcile_published_view();
    assert!(
        app.command_overlay.is_none(),
        "the exited overlay submits Forget"
    );
    settle(&mut app).await;
    server.await.unwrap();
}

#[tokio::test]
async fn plugin_command_shows_the_unavailable_notice() {
    let mut app = app_with(&["t1"]);
    pending(&mut app);
    app.reconcile_published_view();
    app.run_custom_command(&command(CustomCommandAction::PluginAction, "reload"));
    assert!(app.notice.is_some(), "plugin commands show the notice");
    assert!(app.popup.is_none());
}

#[tokio::test]
async fn peek_opens_observes_and_closes() {
    let mut app = app_with(&["t1", "t2"]);
    pending(&mut app);
    app.reconcile_published_view();
    app.open_peek("t2".into());
    let peek = app.peek.as_ref().expect("peek opens");
    assert_eq!(peek.terminal, "t2");
    assert_eq!(peek.pane.target(), Some("t2"));
    // Peeking never takes input; Esc dismisses it.
    key(&mut app, Key::Char('x'));
    assert_eq!(app.peek.as_ref().unwrap().pane.input_backlog().0, 0);
    key(&mut app, Key::Escape);
    assert!(app.peek.is_none());
    assert_eq!(app.focused.as_deref(), Some("t1"));
}

#[tokio::test]
async fn peek_keep_labels_cas_writes_then_keeps() {
    let mut app = app_with_spaces(&[("t1", "work"), ("t2", "work")]);
    pending(&mut app);
    app.reconcile_published_view();
    let mut rx = operations(&mut app);
    core_id(&mut app, "core-fake-1");
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(
            true,
            &[
                "terminal.get",
                "terminal.label",
                "doc.get",
                "doc.set",
                "terminal.keep",
            ],
            &[],
        )
        .await;
        let get = core.read_request().await;
        assert_eq!(get["method"], "terminal.get");
        core.reply(&get["id"], get_reply("t2", 1)).await;
        let label = core.read_request().await;
        assert_eq!(label["method"], "terminal.label");
        assert_eq!(label["params"]["terminal"], "t2");
        assert!(label["params"]["labels"]["herdr.space"].is_string());
        assert!(label["params"]["labels"]["herdr.tab"].is_string());
        core.reply(&label["id"], json!({})).await;
        let get_doc = core.read_request().await;
        assert_eq!(get_doc["method"], "doc.get");
        core.reply(&get_doc["id"], json!({"value": {"schema":1}, "version": 5}))
            .await;
        let set = core.read_request().await;
        assert_eq!(set["method"], "doc.set");
        core.reply(&set["id"], json!({"version": 6})).await;
        let keep = core.read_request().await;
        assert_eq!(keep["method"], "terminal.keep");
        assert_eq!(keep["params"], json!({"terminal":"t2","keep":true}));
        core.reply(&keep["id"], json!({})).await;
    });
    app.open_peek("t2".into());
    key(&mut app, Key::Enter);
    assert!(app.peek.is_none(), "keep dismisses the peek");
    settle(&mut app).await;
    server.await.unwrap();
}

/// A wide-grapheme tail slot carries `CellDiffOption::Skip` while the head
/// covers it, so a later frame where the head narrows re-emits the freed
/// cell — the reference emitter's invalidated-width rule
/// (`render_ansi.rs`: `affected_width` forces the trailing cell out even
/// when visually equal). Without the mark the buffer keeps the pre-head
/// content, the diff sees no change, and a host that orphans the tail
/// keeps stale attributes (the parity popup's stray panel-bg column).
#[tokio::test]
async fn wide_tail_slot_reemits_when_head_narrows() {
    use ratatui::buffer::{Buffer, CellDiffOption};
    use ratatui::layout::Rect;

    let mut app = app_with(&["t1"]);
    pending(&mut app);
    let mut vt = hc_vt::Vt::new(10, 2, 0);
    vt.feed("中".as_bytes());
    app.pane.install_vt(vt);
    let rect = Rect::new(0, 0, 10, 4);
    let mut prev = Buffer::empty(rect);
    super::super::draw::draw_terminal(&mut app.pane, rect, &mut prev, Instant::now(), false);
    super::super::draw::mark_wide_tail_slots(&mut prev);
    assert_eq!(prev[(0, 0)].symbol(), "中");
    assert!(matches!(prev[(1, 0)].diff_option, CellDiffOption::Skip));

    // Next frame: a narrow cell replaces the head; the freed tail must be
    // part of the emitted diff even though both buffers show ' ' there.
    let mut next = Buffer::empty(rect);
    next.set_string(0, 0, "│", ratatui::style::Style::default());
    let emitted: Vec<(u16, u16)> = prev
        .diff(&next)
        .into_iter()
        .map(|(x, y, _)| (x, y))
        .collect();
    assert!(
        emitted.contains(&(1, 0)),
        "tail slot must re-emit: {emitted:?}"
    );
}

use super::*;
use crate::app::input::{MouseButton, MouseEventKind};
use crate::chrome::menu::{ContextMenuAction, ContextMenuTarget};
use crate::chrome::render::{ClientMobileTarget, MachineHit, PaneHit};
use crate::input::KeybindAction as A;
use crate::testcore;
use hc_vt::{Action, Key, Mods};
use ratatui::layout::Rect;
use serde_json::json;

fn pending(app: &mut App) {
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
}

/// Routes layout actions at a fake core answering as `core-fake-1`.
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

fn clear_rename(app: &mut App) {
    app.dialog_key(&term_key(Key::Char('c'), Mods::CTRL, Action::Press));
}

#[tokio::test]
async fn a_workspace_right_click_opens_the_reference_menu_and_rename_writes_the_adoption() {
    let mut app = app_with_spaces(&[("t1", "work"), ("t2", "work")]);
    pending(&mut app);
    draw_to(&mut app, 120, 40);
    let hit = app
        .hit_map
        .workspaces
        .iter()
        .find(|h| h.endpoint_id == ClientEndpointId::Local)
        .unwrap()
        .clone();
    app.context_menu_press((hit.rect.x, hit.rect.y));
    let Some(menu) = &app.menu else {
        panic!("the workspace row opens a context menu")
    };
    let ContextMenuTarget::Workspace { workspace_id, .. } = &menu.target else {
        panic!("the menu targets the clicked workspace")
    };
    assert_eq!(workspace_id, &hit.workspace_id);
    let labels: Vec<_> = app
        .menu
        .as_ref()
        .unwrap()
        .items()
        .iter()
        .map(|i| i.label)
        .collect();
    assert_eq!(labels[..2], ["Rename", "Close"]);
    let mut rx = operations(&mut app);
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["doc.get", "doc.set"], &[]).await;
        let read = core.read_request().await;
        core.reply(&read["id"], json!({"value":null,"version":0}))
            .await;
        let write = core.read_request().await;
        assert_eq!(write["method"], "doc.set");
        let adoption = &write["params"]["value"]["adoptions"]["0:/space:4:work"];
        assert_eq!(adoption["name"], "renamed");
        assert_eq!(adoption["custom_label"], true);
        core.reply(&write["id"], json!({"version":1})).await;
    });
    // Item 0 is "Rename": Enter opens the rename dialog.
    app.menu_key(&press(Key::Enter));
    let Some(dialog) = &app.dialog else {
        panic!("rename opens a dialog")
    };
    let super::super::dialogs::Target::Workspace { .. } = dialog.target else {
        panic!("the dialog persists a workspace adoption")
    };
    clear_rename(&mut app);
    app.insert_dialog_text("renamed");
    app.dialog_key(&press(Key::Enter));
    settle(&mut app).await;
    assert_eq!(app.pending_actions, 0);
    assert!(app.notice.is_none());
    server.await.unwrap();
}

#[tokio::test]
async fn a_machine_connect_activation_reaches_the_wire_without_a_dialog() {
    let mut app = app_with_spaces(&[("t1", "work")]);
    pending(&mut app);
    app.hit_map.machines.push(MachineHit {
        rect: Rect::new(0, 0, 12, 1),
        status_badge: Rect::default(),
        collapse_toggle: Rect::default(),
        endpoint_id: ClientEndpointId::Remote("host-b".into()),
    });
    app.context_menu_press((1, 0));
    let Some(menu) = &app.menu else {
        panic!("the machine row opens a context menu")
    };
    let ContextMenuTarget::Machine { name } = &menu.target else {
        panic!("the menu targets the machine")
    };
    assert_eq!(name.as_deref(), Some("host-b"));
    let actions: Vec<_> = app
        .menu
        .as_ref()
        .unwrap()
        .items()
        .iter()
        .map(|i| i.action)
        .collect();
    assert_eq!(
        actions,
        [
            ContextMenuAction::MachineConnect,
            ContextMenuAction::MachineRename,
            ContextMenuAction::MachineRemove,
        ]
    );
    let mut rx = operations(&mut app);
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["machine.connect"], &[]).await;
        let req = core.read_request().await;
        assert_eq!(req["method"], "machine.connect");
        assert_eq!(req["params"]["machine"], "host-b");
        core.reply(&req["id"], json!({})).await;
    });
    app.menu_key(&press(Key::Enter));
    assert!(app.menu.is_none());
    settle(&mut app).await;
    assert_eq!(app.pending_actions, 0);
    assert!(app.notice.is_none());
    server.await.unwrap();
}

#[tokio::test]
async fn machine_dialogs_submit_add_rename_and_remove() {
    use super::super::dialogs::Target;
    for (open, method, params) in [
        ("add", "machine.add", json!({"target": "alice@host-b"})),
        (
            "rename",
            "machine.rename",
            json!({"machine": "host-b", "new_name": "build-box"}),
        ),
        ("remove", "machine.rm", json!({"machine": "host-b"})),
    ] {
        let mut app = app_with_spaces(&[("t1", "work")]);
        pending(&mut app);
        let mut rx = operations(&mut app);
        let server = tokio::spawn(async move {
            let mut core = rx.recv().await.unwrap();
            core.read_hello().await;
            core.answer_hello(true, &[method], &[]).await;
            let req = core.read_request().await;
            assert_eq!(req["method"], method);
            assert_eq!(req["params"], params);
            core.reply(&req["id"], json!({})).await;
        });
        match open {
            "add" => {
                app.machine_add();
                assert!(matches!(
                    app.dialog.as_ref().unwrap().target,
                    Target::MachineAdd
                ));
                app.insert_dialog_text("alice@host-b");
            }
            "rename" => {
                app.machine_rename("host-b".into());
                assert!(matches!(
                    app.dialog.as_ref().unwrap().target,
                    Target::MachineRename { .. }
                ));
                // The prefilled name is selected: typing replaces it.
                app.insert_dialog_text("build-box");
            }
            _ => {
                app.machine_remove("host-b".into());
                assert!(matches!(
                    app.dialog.as_ref().unwrap().target,
                    Target::MachineRemove { .. }
                ));
            }
        }
        app.dialog_key(&press(Key::Enter));
        settle(&mut app).await;
        assert_eq!(app.pending_actions, 0, "{open}");
        assert!(app.notice.is_none(), "{open}");
        server.await.unwrap();
    }
}

#[tokio::test]
async fn new_workspace_runs_a_kept_terminal_and_records_the_adoption() {
    let mut app = app_with_spaces(&[("t1", "work"), ("t2", "work")]);
    pending(&mut app);
    app.focused = Some("t1".into());
    app.refresh_projection();
    let mut rx = operations(&mut app);
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["doc.get", "doc.set", "terminal.run"], &[])
            .await;
        for _ in 0..2 {
            let read = core.read_request().await;
            assert_eq!(read["method"], "doc.get");
            core.reply(&read["id"], json!({"value":null,"version":0}))
                .await;
        }
        let run = core.read_request().await;
        assert_eq!(run["method"], "terminal.run");
        assert_eq!(run["params"]["keep"], true);
        assert_eq!(run["params"]["no_parent"], true);
        assert_eq!(run["params"]["no_notify"], true);
        assert_eq!(run["params"]["labels"]["herdr.seen"], "0");
        assert!(
            !run["params"]["labels"]["herdr.space"]
                .as_str()
                .unwrap()
                .is_empty()
        );
        let mut terminal = testcore::minimal_terminal();
        terminal["address"] = json!("t9");
        core.reply(&run["id"], terminal).await;
        let read = core.read_request().await;
        core.reply(&read["id"], json!({"value":null,"version":0}))
            .await;
        let write = core.read_request().await;
        assert_eq!(write["method"], "doc.set");
        let adoptions = write["params"]["value"]["adoptions"].as_object().unwrap();
        assert_eq!(adoptions.len(), 1);
        core.reply(&write["id"], json!({"version":1})).await;
    });
    app.on_action(A::NewWorkspace, 80);
    assert_eq!(app.pending_actions, 1);
    settle(&mut app).await;
    assert_eq!(app.pending_actions, 0);
    assert!(app.notice.is_none());
    server.await.unwrap();
}

#[tokio::test]
async fn the_mobile_new_workspace_target_suspends_the_switcher_and_dispatches() {
    let mut app = app_with_spaces(&[("t1", "work")]);
    pending(&mut app);
    app.focused = Some("t1".into());
    app.mode = Mode::Navigate;
    app.refresh_projection();
    let mut rx = operations(&mut app);
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["doc.get", "doc.set", "terminal.run"], &[])
            .await;
        let mut runs = 0;
        while runs == 0 {
            let req = core.read_request().await;
            if req["method"] == "terminal.run" {
                runs += 1;
                assert_eq!(req["params"]["keep"], true);
            }
            let result = match req["method"].as_str().unwrap() {
                "terminal.run" => testcore::minimal_terminal(),
                "doc.set" => json!({"version":1}),
                _ => json!({"value":null,"version":0}),
            };
            core.reply(&req["id"], result).await;
        }
    });
    app.on_mobile_target(&ClientMobileTarget::NewWorkspace);
    assert!(app.mobile_switcher_suspended);
    settle(&mut app).await;
    assert_eq!(app.pending_actions, 0);
    app.resume_mobile_switcher_if_ready();
    assert!(!app.mobile_switcher_suspended);
    server.await.unwrap();
}

#[tokio::test]
async fn escape_and_an_outside_press_close_the_menu() {
    let mut app = app_with_spaces(&[("t1", "work")]);
    pending(&mut app);
    app.hit_map.panes.push(PaneHit {
        rect: Rect::new(0, 0, 20, 10),
        inner_rect: Rect::new(0, 0, 20, 10),
        scrollbar_rect: None,
        scroll: None,
        pane_id: "t1".into(),
    });
    app.context_menu_press((2, 2));
    assert!(app.menu.is_some());
    app.menu_key(&press(Key::Escape));
    assert!(app.menu.is_none());
    app.context_menu_press((2, 2));
    assert!(app.menu.is_some());
    app.menu_mouse(input::MouseInput {
        kind: MouseEventKind::Down(MouseButton::Left),
        col: 60,
        row: 20,
        mods: Mods::NONE,
        sgr_position: None,
    });
    assert!(app.menu.is_none());
}

#[tokio::test]
async fn the_passthrough_toggle_round_trips_through_the_menu() {
    let mut app = app_with_spaces(&[("t1", "work")]);
    pending(&mut app);
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.hit_map.panes.push(PaneHit {
        rect: Rect::new(0, 0, 20, 10),
        inner_rect: Rect::new(0, 0, 20, 10),
        scrollbar_rect: None,
        scroll: None,
        pane_id: "t1".into(),
    });
    app.context_menu_press((2, 2));
    let items = app.menu.as_ref().unwrap().items();
    let toggle = items
        .iter()
        .position(|i| i.action == ContextMenuAction::ToggleRightClickPassthrough)
        .unwrap();
    assert_eq!(items[toggle].label, "Send right-clicks to pane");
    for _ in 0..toggle {
        app.menu_key(&press(Key::ArrowDown));
    }
    app.menu_key(&press(Key::Enter));
    assert!(app.right_click_panes.contains("t1"));
    app.context_menu_press((2, 2));
    let items = app.menu.as_ref().unwrap().items();
    let toggle = items
        .iter()
        .position(|i| i.action == ContextMenuAction::ToggleRightClickPassthrough)
        .unwrap();
    assert_eq!(items[toggle].label, "Use Herdr right-click menu");
}

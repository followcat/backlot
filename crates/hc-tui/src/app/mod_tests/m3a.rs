use super::*;
use crate::{input::KeybindAction as A, operations::Written};
use hc_vt::{Action, Key, Mods};
use serde_json::json;

fn pending(app: &mut App) {
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.sync_target();
}
fn press(app: &mut App, key: Key, mods: Mods, action: Action) {
    app.on_input(UiEvent::Key(term_key(key, mods, action)), 120);
}

#[tokio::test]
async fn lock_forwards_prefix_keys_and_toggle_repeats_remain_consumed() {
    let mut app = app_with_spaces(&[("t1", "work")]);
    pending(&mut app);
    press(&mut app, Key::Char('g'), Mods::CTRL, Action::Press);
    assert!(app.keyboard_locked);
    press(&mut app, Key::Char('g'), Mods::CTRL, Action::Repeat);
    assert!(app.keyboard_locked);
    press(&mut app, Key::Char('b'), Mods::CTRL, Action::Press);
    press(&mut app, Key::Char('c'), Mods::NONE, Action::Press);
    assert!(!app.prefix.armed());
    assert!(app.dialog.is_none());
    assert_eq!(app.pane.input_backlog().0, 2);
    press(&mut app, Key::Char('g'), Mods::CTRL, Action::Release);
    press(&mut app, Key::Char('g'), Mods::CTRL, Action::Press);
    assert!(!app.keyboard_locked);
    let screen = screen_lines(&draw_to(&mut app, 120, 40)).join("\n");
    assert!(!screen.contains("LOCKED"));
}

#[tokio::test]
async fn dialog_text_and_paste_never_reach_the_program_and_fit_mobile() {
    let mut app = app_with_spaces(&[("t1", "work")]);
    pending(&mut app);
    app.on_action(A::NewTab, 120);
    assert!(app.dialog.is_some());
    app.on_input(UiEvent::Paste(b"next\nname".to_vec()), 120);
    app.on_input(UiEvent::Text("!".into()), 120);
    let super::super::dialogs::Body::Rename(rename) = &app.dialog.as_ref().unwrap().body else {
        panic!()
    };
    assert_eq!(rename.input.as_str(), "next name!");
    assert_eq!(app.pane.input_backlog().0, 0);
    for width in [40, 64, 80, 120, 200] {
        let backend = draw_to(&mut app, width, 24);
        assert!(screen_lines(&backend).join("\n").contains("new tab"));
        let cursor = app.want_cursor.pos.unwrap();
        assert!(cursor.0 < width && cursor.1 < 24);
    }
    press(&mut app, Key::Escape, Mods::NONE, Action::Press);
    assert!(app.dialog.is_none());
}

#[tokio::test]
async fn confirmed_write_survives_older_watcher_and_pending_focus_does_not_roll_back() {
    let mut app = app_with_spaces(&[("t1", "work"), ("t2", "work")]);
    pending(&mut app);
    app.pending_actions = 2;
    app.on_operation(Some(Ok(Some(Written {
        core: "id-host-a".into(),
        document: crate::structure::Document::decode(json!({"schema":1,"focus":"t2"})).unwrap(),
        version: 5,
    }))));
    assert_eq!(app.focused.as_deref(), Some("t1"));
    app.view.documents.insert(
        super::super::shared::key(),
        hc_proto::watch::Document {
            value: json!(null),
            version: 4,
            ..Default::default()
        },
    );
    app.retain_confirmed_layout();
    assert_eq!(app.view.documents[&super::super::shared::key()].version, 5);
    app.on_operation(Some(Ok(None)));
    app.reconcile_published_view();
    assert_eq!(app.focused.as_deref(), Some("t2"));
    app.view.core.as_mut().unwrap().id = "replacement".into();
    app.retain_confirmed_layout();
    assert!(app.layout_confirmed.is_none());
}

#[tokio::test]
async fn directional_navigation_and_cycle_use_reference_tree_order() {
    let mut app = app_with_spaces(&[("t1", "work"), ("t2", "work"), ("t3", "work")]);
    pending(&mut app);
    app.on_action(A::FocusPaneRight, 120);
    assert_eq!(app.focused.as_deref(), Some("t2"));
    app.on_action(A::CyclePanePrevious, 120);
    assert_eq!(app.focused.as_deref(), Some("t1"));
    app.on_action(A::CyclePanePrevious, 120);
    assert_eq!(app.focused.as_deref(), Some("t3"));
    app.on_action(A::EnterResizeMode, 120);
    assert_eq!(app.mode, Mode::Resize);
    press(&mut app, Key::Escape, Mods::NONE, Action::Press);
    assert_eq!(app.mode, Mode::Normal);
}

#[tokio::test]
async fn configured_direct_action_and_prefix_use_the_same_dispatcher() {
    let config: crate::config::Config =
        toml::from_str("[keys]\nprefix='ctrl+a'\nnew_tab='alt+c'\n").unwrap();
    let mut app = app_with_spaces(&[("t1", "work")]);
    pending(&mut app);
    app.live = config.live_keybinds();
    press(&mut app, Key::Char('c'), Mods::ALT, Action::Press);
    assert!(app.dialog.is_some());
    press(&mut app, Key::Escape, Mods::NONE, Action::Press);
    press(&mut app, Key::Char('a'), Mods::CTRL, Action::Press);
    assert!(app.prefix.armed());
    press(&mut app, Key::Char('r'), Mods::NONE, Action::Press);
    assert_eq!(app.mode, Mode::Resize);
    assert!(!app.prefix.armed());
}

#[tokio::test]
async fn split_drag_grabs_last_painted_topology_and_releases_outside_the_pane() {
    let mut app = app_with_spaces(&[("t1", "work"), ("t2", "work")]);
    pending(&mut app);
    app.area = (120, 40);
    app.sync_observers();
    draw_to(&mut app, 120, 40);
    let hit = app.hit_map.pane_splits.first().unwrap().clone();
    assert!(app.grab_split((hit.rect.x, hit.rect.y + 1)));
    let super::super::mouse::ChromeDrag::PaneSplit { edit, .. } = app.chrome_drag.as_ref().unwrap()
    else {
        panic!()
    };
    let crate::operations::Change::Ratio { expected, path, .. } = &edit.change else {
        panic!()
    };
    assert_eq!(expected, &hit.root);
    assert_eq!(path, &hit.split.path);
    app.on_mouse(input::MouseInput {
        kind: input::MouseEventKind::Up(input::MouseButton::Left),
        col: 119,
        row: 39,
        mods: Mods::NONE,
        sgr_position: None,
    });
    assert!(app.chrome_drag.is_none());
}

#[tokio::test]
async fn zoom_keeps_multi_pane_outer_borders_and_resize_mode_uses_base_keyboard_flags() {
    let mut app = app_with_spaces(&[("t1", "work"), ("t2", "work")]);
    pending(&mut app);
    let tab = app.projection.snapshot.focused_tab_id.clone().unwrap();
    app.projection.layouts.get_mut(&tab).unwrap().zoomed = Some("t1".into());
    let infos =
        super::super::pane_geometry::pane_infos(&app, ratatui::layout::Rect::new(26, 0, 94, 39));
    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].borders, ratatui::widgets::Borders::ALL);
    app.mode = Mode::Resize;
    app.host_reports_all_keys = true;
    app.sync_keyboard_report_all();
    assert!(!app.host_reports_all_keys);
}

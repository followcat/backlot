use super::*;
use crate::input::KeybindAction as A;
use hc_vt::{Action, Key, Mods};
use serde_json::json;
fn pending(app: &mut App) {
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.sync_target();
}
fn tabs(n: usize) -> App {
    let terms = (1..=n)
        .map(|n| {
            let mut t = crate::sidebar::fixture::term(&format!("t{n}"));
            t.pid = Some(1);
            t.labels.insert("herdr.space".into(), "work".into());
            t.labels.insert("herdr.tab".into(), format!("tab{n}"));
            t
        })
        .collect();
    app_with_terms(terms)
}
fn mouse(kind: input::MouseEventKind, x: u16, y: u16) -> input::MouseInput {
    input::MouseInput {
        kind,
        col: x,
        row: y,
        mods: Mods::NONE,
        sgr_position: None,
    }
}
#[tokio::test]
async fn an_untracked_lock_repeat_is_consumed_after_focus_loss() {
    let mut app = app_with_spaces(&[("t1", "work")]);
    pending(&mut app);
    for locked in [false, true] {
        app.keyboard_locked = locked;
        app.on_input(UiEvent::Focus(false), 120);
        app.on_input(
            UiEvent::Key(term_key(Key::Char('g'), Mods::CTRL, Action::Repeat)),
            120,
        );
        assert_eq!(app.keyboard_locked, locked);
        assert_eq!(app.pane.input_backlog().0, 0);
    }
}
#[tokio::test]
async fn every_final_operation_result_reconciles_skipped_focus_without_another_publish() {
    for result in [
        Ok(crate::operations::Outcome::Done),
        Err("fixture failure".into()),
    ] {
        let mut app = app_with_spaces(&[("t1", "work"), ("t2", "work")]);
        pending(&mut app);
        app.pending_actions = 1;
        app.view.documents.insert(
            super::super::shared::key(),
            hc_proto::watch::Document {
                value: json!({"schema":1,"focus":"t2"}),
                version: 5,
                ..Default::default()
            },
        );
        app.on_operation(Some(result));
        assert_eq!(app.focused.as_deref(), Some("t2"));
    }
}
#[tokio::test]
async fn a_moved_confirmation_target_is_rejected_and_mouse_cancel_stays_in_terminal_mode() {
    let mut app = app_with_spaces(&[("t1", "work")]);
    pending(&mut app);
    app.close_layout(false);
    let super::super::dialogs::Target::Close(action) = &app.dialog.as_ref().unwrap().target else {
        panic!()
    };
    assert!(app.close_membership_valid(action));
    let mut term = app.view.inventory.get("t1").unwrap().clone();
    term.labels.insert("herdr.tab".into(), "moved".into());
    app.view.inventory.replace(2, vec![term]);
    app.refresh_projection();
    let super::super::dialogs::Target::Close(action) = &app.dialog.as_ref().unwrap().target else {
        panic!()
    };
    assert!(!app.close_membership_valid(action));
    draw_to(&mut app, 120, 40);
    let cancel = app.dialog_hits.cancel;
    app.dialog_mouse(mouse(
        input::MouseEventKind::Down(input::MouseButton::Left),
        cancel.x,
        cancel.y,
    ));
    assert!(app.dialog.is_none());
    assert_eq!(app.mode, Mode::Normal);
    app.close_layout(false);
    draw_to(&mut app, 120, 40);
    app.dialog_mouse(mouse(
        input::MouseEventKind::Down(input::MouseButton::Left),
        0,
        0,
    ));
    assert!(app.dialog.is_none());
    assert_eq!(app.mode, Mode::Normal);
}
#[tokio::test]
async fn focused_tabs_are_revealed_and_one_cell_drag_outside_does_not_focus_or_reorder() {
    let mut app = tabs(12);
    pending(&mut app);
    app.area = (80, 24);
    draw_to(&mut app, 80, 24);
    app.on_action(A::SwitchTab(11), 80);
    assert!(app.shell_state.reveal_focused_tab);
    draw_to(&mut app, 80, 24);
    let target = app.projection.snapshot.focused_tab_id.clone().unwrap();
    assert!(app.hit_map.tabs.iter().any(|(_, id)| id == &target));
    app.shell_state.tab_scroll = 0;
    app.focused = Some("t1".into());
    app.refresh_projection();
    draw_to(&mut app, 80, 24);
    let rect = app.hit_map.tabs[1].0;
    app.mouse_press((rect.x, rect.y));
    app.mouse_drag((rect.x + 1, rect.y));
    assert!(app.tab_dragged);
    app.mouse_drag((rect.x + 1, rect.y + 5));
    app.mouse_release((rect.x + 1, rect.y + 5));
    assert_eq!(app.focused.as_deref(), Some("t1"));
    assert!(app.notice.is_none());
}
#[tokio::test]
async fn overflow_drop_arrows_mean_the_entire_tab_list_and_old_core_drag_is_cancelled() {
    let mut app = tabs(3);
    pending(&mut app);
    draw_to(&mut app, 120, 40);
    app.hit_map.tab_scroll_left = ratatui::layout::Rect::new(0, 0, 3, 1);
    app.hit_map.tab_scroll_right = ratatui::layout::Rect::new(117, 0, 3, 1);
    assert_eq!(app.tab_drop_index((1, 0)), Some(0));
    assert_eq!(app.tab_drop_index((118, 0)), Some(3));
    let mut app = app_with_spaces(&[("t1", "work"), ("t2", "work")]);
    pending(&mut app);
    app.area = (120, 40);
    draw_to(&mut app, 120, 40);
    let hit = app.hit_map.pane_splits[0].clone();
    assert!(app.grab_split((hit.rect.x, hit.rect.y + 1)));
    app.view.core.as_mut().unwrap().id = "replacement".into();
    app.drag_split((hit.rect.x + 3, hit.rect.y + 1), true);
    assert!(app.chrome_drag.is_none());
    assert!(app.notice.is_none());
}

#[tokio::test]
async fn relative_agent_focus_preserves_scroll_for_visible_endpoint_rows() {
    let terms = ["t1", "t2"]
        .into_iter()
        .map(|id| {
            let mut t = crate::sidebar::fixture::term(id);
            t.pid = Some(1);
            t.status = Some(hc_proto::terminal::Status {
                state: "working".into(),
                app: "fixture".into(),
                ..Default::default()
            });
            t
        })
        .collect();
    let mut app = app_with_terms(terms);
    pending(&mut app);
    app.hit_map.agent_max_scroll = 50;
    app.shell_state.agent_scroll = 7;
    app.hit_map.agents.clear();
    app.hit_map.endpoint_agents.push((
        ratatui::layout::Rect::default(),
        ClientEndpointId::Local,
        "t2".into(),
    ));
    app.on_action(A::NextAgent, 120);
    assert_eq!(app.focused.as_deref(), Some("t2"));
    assert_eq!(app.shell_state.agent_scroll, 7);
    app.hit_map.endpoint_agents.clear();
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.on_action(A::NextAgent, 120);
    assert_eq!(app.shell_state.agent_scroll, 1);
}

#[tokio::test]
async fn an_empty_inventory_never_opens_a_size_holder_for_an_empty_address() {
    let mut app = app();
    let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let now = Instant::now();
    app.sched.mark_dirty(now);
    app.maybe_draw(&mut term, now).unwrap();
    assert!(app.size_leases.is_empty());
    assert_eq!(app.pane.attaches, 0);
}
#[tokio::test]
async fn a_core_replacement_retires_pending_tab_and_workspace_gestures() {
    let mut app = tabs(3);
    pending(&mut app);
    app.reconcile_published_view();
    draw_to(&mut app, 120, 40);
    let rect = app.hit_map.tabs[1].0;
    app.mouse_press((rect.x, rect.y));
    app.mouse_drag((rect.x + 1, rect.y));
    assert!(app.tab_press.is_some());
    assert!(app.tab_dragged);
    app.view.core.as_mut().unwrap().id = "replacement".into();
    app.reconcile_published_view();
    assert!(app.tab_press.is_none());
    assert!(!app.tab_dragged);
    assert!(app.chrome_drag.is_none());
}

//! M4 slice 3: the navigator ("Go to") overlay.
use super::*;
use crate::app::input::{MouseButton, MouseEventKind};
use crate::chrome::navigator::{NavigatorFilter, NavigatorTarget};
use crate::input::KeybindAction as A;
use crate::testcore;
use hc_vt::{Action, Key, Mods};
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

fn key(app: &mut App, key: Key) {
    app.on_input(
        input::UiEvent::Key(term_key(key, Mods::NONE, Action::Press)),
        120,
    );
}

fn mouse(app: &mut App, kind: MouseEventKind, col: u16, row: u16) {
    app.on_input(
        input::UiEvent::Mouse(input::MouseInput {
            kind,
            col,
            row,
            mods: Mods::NONE,
            sgr_position: None,
        }),
        120,
    );
}

/// `t1`/`t2` share `work`, `t3` is alone in `other`; `t1` is the
/// reported agent, `t2`/`t3` are shell terminals.
fn app_nav() -> App {
    let mut app = app();
    let mut t1 = crate::sidebar::fixture::agent("t1", "working");
    t1.labels.insert("herdr.space".into(), "work".into());
    t1.labels.insert("herdr.tab".into(), "tab".into());
    let mut t2 = crate::sidebar::fixture::term("t2");
    t2.labels.insert("herdr.space".into(), "work".into());
    t2.labels.insert("herdr.tab".into(), "tab".into());
    let mut t3 = crate::sidebar::fixture::term("t3");
    t3.labels.insert("herdr.space".into(), "other".into());
    t3.labels.insert("herdr.tab".into(), "tab".into());
    app.view = crate::sidebar::fixture::view(vec![t1, t2, t3], vec![]);
    app.focused = Some("t1".into());
    pending(&mut app);
    app.reconcile_published_view();
    app
}

fn open(app: &mut App) {
    app.on_action(A::OpenNavigator, 120);
    assert!(app.navigator.is_some());
}

fn screen(app: &mut App) -> Vec<String> {
    screen_lines(&draw_to(app, 120, 40))
}

#[tokio::test]
async fn prefix_g_opens_the_navigator_with_the_current_pane_selected() {
    let mut app = app_nav();
    // The real route: prefix arms, `g` resolves OpenNavigator.
    app.on_input(
        input::UiEvent::Key(term_key(Key::Char('b'), Mods::CTRL, Action::Press)),
        120,
    );
    key(&mut app, Key::Char('g'));
    assert!(app.navigator.is_some());
    let navigator = app.navigator.as_ref().unwrap();
    assert_eq!(
        navigator.selected,
        Some(NavigatorTarget::Pane {
            endpoint_id: ClientEndpointId::Local,
            pane_id: "t1".into(),
        })
    );
    let lines = screen(&mut app);
    let body = lines.join("\n");
    assert!(body.contains(" Go to "), "title: {body}");
    assert!(body.contains("/ search agents and terminals"), "{body}");
    assert!(body.contains("3 terminals"), "{body}");
    assert!(body.contains("work"), "{body}");
    assert!(body.contains("other"), "{body}");
    // The current pane carries the ◆ marker.
    assert!(body.contains('◆'), "{body}");
    key(&mut app, Key::Escape);
    assert!(app.navigator.is_none());
}

#[tokio::test]
async fn the_status_filters_keep_matching_panes() {
    let mut app = app_nav();
    open(&mut app);
    // `b` → blocked only: none of the three panes is blocked.
    key(&mut app, Key::Char('b'));
    assert_eq!(
        app.navigator.as_ref().unwrap().filter,
        Some(NavigatorFilter::Blocked)
    );
    let lines = screen(&mut app);
    let body = lines.join("\n");
    assert!(body.contains("/ blocked"), "{body}");
    assert!(body.contains("0 terminals"), "{body}");
    assert!(body.contains("No matching agents or terminals"), "{body}");
    // `w` → working: only the `t1` agent.
    key(&mut app, Key::Char('w'));
    let body = screen(&mut app).join("\n");
    assert!(body.contains("/ working"), "{body}");
    assert!(body.contains("1 terminal"), "{body}");
    // `i`/`d` show empty result lists too (idle/done words absent).
    key(&mut app, Key::Char('i'));
    let body = screen(&mut app).join("\n");
    assert!(body.contains("/ idle"), "{body}");
    assert!(body.contains("0 terminals"), "{body}");
    key(&mut app, Key::Char('d'));
    let body = screen(&mut app).join("\n");
    assert!(body.contains("/ done"), "{body}");
    assert!(body.contains("0 terminals"), "{body}");
    // Backspace drops the active filter; `a` also clears it.
    key(&mut app, Key::Char('w'));
    key(&mut app, Key::Backspace);
    assert_eq!(app.navigator.as_ref().unwrap().filter, None);
    let body = screen(&mut app).join("\n");
    assert!(body.contains("3 terminals"), "{body}");
    key(&mut app, Key::Char('w'));
    key(&mut app, Key::Char('a'));
    assert_eq!(app.navigator.as_ref().unwrap().filter, None);
}

#[tokio::test]
async fn search_matches_workspace_and_pane_text() {
    let mut app = app_nav();
    open(&mut app);
    key(&mut app, Key::Char('/'));
    assert!(app.navigator.as_ref().unwrap().search_focused);
    // "oth" matches the `other` workspace and its pane — the workspace
    // heading stays as ancestor context.
    app.on_input(input::UiEvent::Text("oth".into()), 120);
    let body = screen(&mut app).join("\n");
    assert!(body.contains("/ oth"), "{body}");
    assert!(body.contains("1 terminal"), "{body}");
    assert!(body.contains("other"), "{body}");
    assert!(!body.contains("◆"), "{body}");
    // A query with no hits lists the empty message.
    app.on_input(input::UiEvent::Text("zzz".into()), 120);
    let body = screen(&mut app).join("\n");
    assert!(body.contains("0 terminals"), "{body}");
    assert!(body.contains("No matching agents or terminals"), "{body}");
    // Esc backs out of search without closing.
    key(&mut app, Key::Escape);
    assert!(app.navigator.is_some());
    assert!(!app.navigator.as_ref().unwrap().search_focused);
    key(&mut app, Key::Escape);
    assert!(app.navigator.is_none());
}

#[tokio::test]
async fn arrows_and_page_keys_move_the_selection() {
    let mut app = app_nav();
    open(&mut app);
    // Rows: work heading, t1 pane, t2 pane, other heading, t3 pane.
    key(&mut app, Key::Char('j'));
    assert_eq!(
        app.navigator.as_ref().unwrap().selected,
        Some(NavigatorTarget::Pane {
            endpoint_id: ClientEndpointId::Local,
            pane_id: "t2".into(),
        })
    );
    key(&mut app, Key::ArrowUp);
    assert_eq!(
        app.navigator.as_ref().unwrap().selected,
        Some(NavigatorTarget::Pane {
            endpoint_id: ClientEndpointId::Local,
            pane_id: "t1".into(),
        })
    );
    // End selects the last row; Home clears back to the first pane.
    key(&mut app, Key::End);
    assert_eq!(
        app.navigator.as_ref().unwrap().selected,
        Some(NavigatorTarget::Pane {
            endpoint_id: ClientEndpointId::Local,
            pane_id: "t3".into(),
        })
    );
    key(&mut app, Key::Home);
    assert_eq!(app.navigator.as_ref().unwrap().selected, None);
    // → hops to the next workspace section's first pane (t3), ← back.
    key(&mut app, Key::ArrowRight);
    assert_eq!(
        app.navigator.as_ref().unwrap().selected,
        Some(NavigatorTarget::Pane {
            endpoint_id: ClientEndpointId::Local,
            pane_id: "t3".into(),
        })
    );
    key(&mut app, Key::ArrowLeft);
    assert_eq!(
        app.navigator.as_ref().unwrap().selected,
        Some(NavigatorTarget::Pane {
            endpoint_id: ClientEndpointId::Local,
            pane_id: "t1".into(),
        })
    );
}

#[tokio::test]
async fn wheel_and_long_lists_scroll_the_row_window() {
    // Twenty panes in one workspace — taller than the 15-row popup body
    // at 80x24, so the scrollbar and scroll clamp engage.
    let mut app = app();
    let terms: Vec<_> = (1..=20)
        .map(|n| {
            let mut t = crate::sidebar::fixture::term(&format!("t{n}"));
            t.labels.insert("herdr.space".into(), "work".into());
            t.labels.insert("herdr.tab".into(), "tab".into());
            t.created_ms = n;
            t
        })
        .collect();
    app.view = crate::sidebar::fixture::view(terms, vec![]);
    app.focused = Some("t1".into());
    pending(&mut app);
    app.reconcile_published_view();
    open(&mut app);
    draw_to(&mut app, 80, 24);
    assert!(app.navigator_hits.scrollbar.height > 0);
    let last = app
        .navigator_hits
        .rows
        .last()
        .map(|(_, target)| target.clone());
    // The viewport does not reach the final pane before scrolling.
    assert_ne!(
        last,
        Some(NavigatorTarget::Pane {
            endpoint_id: ClientEndpointId::Local,
            pane_id: "t20".into(),
        })
    );
    // The wheel steps the selection by three, like the reference.
    mouse(&mut app, MouseEventKind::ScrollDown, 10, 10);
    assert_eq!(
        app.navigator.as_ref().unwrap().selected,
        Some(NavigatorTarget::Pane {
            endpoint_id: ClientEndpointId::Local,
            pane_id: "t4".into(),
        })
    );
    // Ctrl+d pages eight rows.
    app.on_input(
        input::UiEvent::Key(term_key(Key::Char('d'), Mods::CTRL, Action::Press)),
        80,
    );
    assert_eq!(
        app.navigator.as_ref().unwrap().selected,
        Some(NavigatorTarget::Pane {
            endpoint_id: ClientEndpointId::Local,
            pane_id: "t12".into(),
        })
    );
    // End selects the last row; the render's clamp scrolls the
    // viewport so it becomes visible.
    key(&mut app, Key::End);
    draw_to(&mut app, 80, 24);
    assert_eq!(
        app.navigator_hits.rows.last().map(|(_, t)| t.clone()),
        Some(NavigatorTarget::Pane {
            endpoint_id: ClientEndpointId::Local,
            pane_id: "t20".into(),
        })
    );
}

#[tokio::test]
async fn the_mouse_selects_focuses_search_and_dismisses() {
    let mut app = app_nav();
    open(&mut app);
    draw_to(&mut app, 120, 40);
    // Hovering a row moves the selection to it.
    let (rect, target) = app.navigator_hits.rows[2].clone();
    mouse(&mut app, MouseEventKind::Moved, rect.x + 2, rect.y);
    assert_eq!(app.navigator.as_ref().unwrap().selected, Some(target));
    // Clicking the search line focuses the editor and clears filters.
    let search = app.navigator_hits.search;
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        search.x + 2,
        search.y,
    );
    assert!(app.navigator.as_ref().unwrap().search_focused);
    // Unfocus, then a click outside the popup closes it.
    app.navigator.as_mut().unwrap().search_focused = false;
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 0, 0);
    assert!(app.navigator.is_none());
}

#[tokio::test]
async fn a_row_click_accepts_and_focuses_the_pane() {
    let mut app = app_nav();
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
        let value = write["params"]["value"].to_string();
        assert!(value.contains("\"t2\""), "focus write: {value}");
        core.reply(&write["id"], json!({"version":1})).await;
    });
    open(&mut app);
    draw_to(&mut app, 120, 40);
    let (rect, target) = app
        .navigator_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, NavigatorTarget::Pane { pane_id, .. } if pane_id == "t2"))
        .unwrap()
        .clone();
    assert_eq!(
        target,
        NavigatorTarget::Pane {
            endpoint_id: ClientEndpointId::Local,
            pane_id: "t2".into(),
        }
    );
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 2,
        rect.y,
    );
    assert!(app.navigator.is_none());
    assert_eq!(app.focused.as_deref(), Some("t2"));
    let result = tokio::time::timeout(
        testcore::DEADLINE,
        App::operation_result(&mut app.operations),
    )
    .await
    .unwrap();
    app.on_operation(result);
    server.await.unwrap();
}

#[tokio::test]
async fn enter_focuses_the_selected_pane_and_closes() {
    let mut app = app_nav();
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
        core.reply(&write["id"], json!({"version":1})).await;
    });
    open(&mut app);
    key(&mut app, Key::Char('j'));
    key(&mut app, Key::Enter);
    assert!(app.navigator.is_none());
    assert_eq!(app.focused.as_deref(), Some("t2"));
    let result = tokio::time::timeout(
        testcore::DEADLINE,
        App::operation_result(&mut app.operations),
    )
    .await
    .unwrap();
    app.on_operation(result);
    server.await.unwrap();
}

#[tokio::test]
async fn typed_text_never_leaks_to_the_pane_while_open() {
    let mut app = app_nav();
    open(&mut app);
    // A plain character with the overlay unfocused is a shortcut, not text.
    key(&mut app, Key::Char('j'));
    // Paste while open but unfocused is swallowed by the overlay.
    app.on_input(input::UiEvent::Paste(b"echo hi".to_vec()), 120);
    assert!(app.navigator.is_some());
    assert!(app.navigator.as_ref().unwrap().query.is_empty());
    key(&mut app, Key::Char('/'));
    app.on_input(input::UiEvent::Paste(b"work".to_vec()), 120);
    assert_eq!(app.navigator.as_ref().unwrap().query.as_str(), "work");
}

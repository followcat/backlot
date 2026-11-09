//! Panel mouse and animation: title/row/outside clicks, the wheel,
//! the scrollbar drag and the working-mark deadline.
use super::*;

#[tokio::test]
async fn a_title_click_flips_collapsed_and_expanded() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    draw_to(&mut app, 120, 24);
    app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 100, 0));
    assert_eq!(app.panel.mode, PanelMode::Collapsed);
    app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 100, 0));
    assert_eq!(app.panel.mode, PanelMode::Expanded);
}

#[tokio::test]
async fn a_row_click_selects_and_acts_like_enter() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    draw_to(&mut app, 120, 24);
    let alice = app
        .panel_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Person(_, r) if r == "actor-alice"))
        .map(|(r, _)| *r)
        .expect("alice hit row");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        alice.x,
        alice.y,
    ));
    assert_eq!(app.focused.as_deref(), Some("t4"));
}

#[tokio::test]
async fn a_click_outside_a_focused_panel_returns_focus_to_the_panes() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    draw_to(&mut app, 120, 24);
    // Column 0 is far left of the 30-column panel.
    app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 0, 10));
    assert!(!app.panel.focused);
    assert_eq!(app.panel.mode, PanelMode::Expanded);
}

#[tokio::test]
async fn the_wheel_scrolls_the_body() {
    let mut app = app_with_panel_doc();
    app.view.documents.insert(doc_key(), overflow_doc());
    app.area = (120, 12);
    open_panel(&mut app);
    draw_to(&mut app, 120, 12);
    assert!(app.panel_hits.metrics.unwrap().max_offset_from_bottom > 0);
    app.on_mouse(mouse(MouseEventKind::ScrollDown, 100, 5));
    assert_eq!(app.panel.scroll, 1);
    app.on_mouse(mouse(MouseEventKind::ScrollUp, 100, 5));
    assert_eq!(app.panel.scroll, 0);
}

#[tokio::test]
async fn the_scrollbar_drags_the_body() {
    let mut app = app_with_panel_doc();
    app.view.documents.insert(doc_key(), overflow_doc());
    app.area = (120, 12);
    open_panel(&mut app);
    draw_to(&mut app, 120, 12);
    let track = app.panel_hits.scrollbar;
    assert!(!track.is_empty(), "overflow draws the track");
    // Press on the thumb, drag down: the body scrolls down.
    let thumb_top = track.y;
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        track.x,
        thumb_top,
    ));
    app.on_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        track.x,
        thumb_top + 4,
    ));
    app.on_mouse(mouse(
        MouseEventKind::Up(MouseButton::Left),
        track.x,
        thumb_top + 4,
    ));
    assert!(app.panel.scroll > 0, "drag scrolled the body");
}

#[tokio::test]
async fn the_animation_deadline_only_arms_while_something_works() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    // Closed: nothing arms.
    draw_to(&mut app, 120, 24);
    assert!(app.panel_anim_deadline.is_none());
    // Open with working people: armed.
    open_panel(&mut app);
    draw_to(&mut app, 120, 24);
    assert!(
        app.panel_anim_deadline.is_some(),
        "working marks arm the tick"
    );
    // The tick advances the frame once; the next draw re-arms the
    // one-shot deadline without ticking again.
    let due = app.panel_anim_deadline.unwrap();
    app.tick_panel(due);
    let spin = app.panel.spin;
    assert_eq!(spin, 1);
    draw_to(&mut app, 120, 24);
    assert!(app.panel_anim_deadline.is_some());
    assert_eq!(app.panel.spin, spin);
    // Nothing working: no deadline.
    let mut doc = summary_json();
    for role in doc["studios"][0]["roles"].as_array_mut().unwrap() {
        role["work"] = serde_json::Value::Null;
    }
    app.view.documents.insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        },
    );
    draw_to(&mut app, 120, 24);
    assert!(app.panel_anim_deadline.is_none(), "idle panel arms nothing");
    // Closed again: still nothing.
    app.panel.mode = PanelMode::Closed;
    draw_to(&mut app, 120, 24);
    assert!(app.panel_anim_deadline.is_none());
}

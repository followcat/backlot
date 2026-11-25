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
        alice.x + 2,
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
    // Column 0 is far left of the 26-column panel.
    app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 0, 10));
    assert!(!app.panel.focused);
    assert_eq!(app.panel.mode, PanelMode::Expanded);
}

#[tokio::test]
async fn a_closed_persons_click_notices_instead_of_reopening() {
    // R3-M11: a resumable closed seat's click is its Enter — the
    // closed notice, never a respawn; `o` owns the reopen.
    let mut app = app_with_panel_doc();
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_panel_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(()) })
    }));
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.closed_folds.insert("acme-shop".into());
    draw_to(&mut app, 120, 24);
    let dave = app
        .panel_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Person(_, r) if r == "actor-dave"))
        .map(|(r, _)| *r)
        .expect("dave's hit row");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        dave.x + 2,
        dave.y,
    ));
    assert!(
        recorded.lock().unwrap().is_empty(),
        "Enter/click never runs reopen"
    );
    assert!(app.notice.is_some(), "the closed notice names `o`");
}

#[tokio::test]
async fn dragging_the_left_border_resizes_the_dock() {
    // S5 §2: the beside dock's left border is a resize handle —
    // session-local, clamped to `28..=48`.
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    draw_to(&mut app, 120, 24);
    let edge = app.panel_hits.area.x;
    app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), edge, 10));
    assert!(
        matches!(
            app.chrome_drag,
            Some(crate::app::mouse::ChromeDrag::PanelWidth)
        ),
        "the border grab"
    );
    // Drag left: 120 - 80 = 40 columns wide.
    app.on_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 80, 10));
    assert_eq!(app.panel.drag_width, Some(40));
    // The drag width wins over the tiered one at the next draw.
    assert_eq!(app.panel_width(120), 40);
    // Past the max it clamps to 48; at the screen's right edge to 28.
    app.on_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 60, 10));
    assert_eq!(app.panel.drag_width, Some(48));
    app.on_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 118, 10));
    assert_eq!(app.panel.drag_width, Some(28));
    app.on_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 118, 10));
    assert!(app.chrome_drag.is_none(), "released");
}

#[tokio::test]
async fn an_overlay_docks_edge_is_not_a_handle() {
    // Narrow screens draw the dock over the panes — its edge is a
    // border, not a handle (S5 §2).
    let mut app = app_with_panel_doc();
    app.area = (50, 24);
    open_panel(&mut app);
    draw_to(&mut app, 50, 24);
    assert!(!app.panel_beside(50), "the dock overlays at 50");
    let edge = app.panel_hits.area.x;
    app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), edge, 10));
    assert!(
        !matches!(
            app.chrome_drag,
            Some(crate::app::mouse::ChromeDrag::PanelWidth)
        ),
        "no resize handle over the panes"
    );
    assert_eq!(app.panel.drag_width, None);
}

#[tokio::test]
async fn the_wheel_scrolls_the_body() {
    let mut app = app_with_panel_doc();
    app.view
        .documents_mut()
        .insert(doc_key(), overflow_doc().into());
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
    app.view
        .documents_mut()
        .insert(doc_key(), overflow_doc().into());
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
async fn the_wheel_reaches_the_bottom() {
    let mut app = app_with_panel_doc();
    // One-line rows now — a tall cast still overflows a short body:
    // scroll is a line offset bounded by the drawn metrics, so the
    // wheel lands exactly on the bottom.
    let mut value = summary_json();
    let roles = value["studios"][0]["roles"].as_array_mut().unwrap();
    for i in 0..16 {
        roles.push(serde_json::json!({
            "role": format!("actor-t{i}"), "name": format!("t{i}"),
            "state": "live", "work": "waiting",
        }));
    }
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value,
            ..Document::default()
        }
        .into(),
    );
    app.area = (120, 10);
    open_panel(&mut app);
    draw_to(&mut app, 120, 10);
    let max = app.panel_hits.metrics.unwrap().max_offset_from_bottom;
    assert!(max > 0, "the cast overflows the body");
    let body = app.panel_hits.body;
    for _ in 0..max + 4 {
        app.on_mouse(mouse(MouseEventKind::ScrollDown, body.x, body.y));
    }
    assert_eq!(app.panel.scroll, max, "the wheel reached the bottom");
}

/// `draw` with a caller-chosen instant — the animation test steps
/// time frame by frame.
fn draw_at(app: &mut App, w: u16, h: u16, t: std::time::Instant) {
    let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    term.draw(|f| crate::app::draw::draw(app, f, t)).unwrap();
}

#[tokio::test]
async fn a_redraw_does_not_push_the_spin_deadline_out() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    let t0 = std::time::Instant::now();
    let due = t0 + crate::app::panel::PANEL_SPIN;
    // Frames every 16 ms draw a working mark each time; the armed
    // deadline must stay put instead of moving with every redraw.
    for i in 0..8u64 {
        draw_at(
            &mut app,
            120,
            24,
            t0 + std::time::Duration::from_millis(16 * i),
        );
        assert_eq!(app.panel_anim_deadline, Some(due), "frame {i} moved it");
        assert_eq!(app.panel.spin, 0, "frame {i} ticked early");
    }
    // Past the deadline the tick advances the frame and the next draw
    // arms the next one-shot.
    app.tick_panel(due);
    assert_eq!(app.panel.spin, 1);
    let later = due + std::time::Duration::from_millis(16);
    draw_at(&mut app, 120, 24, later);
    assert_eq!(
        app.panel_anim_deadline,
        Some(later + crate::app::panel::PANEL_SPIN)
    );
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
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    draw_to(&mut app, 120, 24);
    assert!(app.panel_anim_deadline.is_none(), "idle panel arms nothing");
    // Closed again: still nothing.
    app.panel.mode = PanelMode::Closed;
    draw_to(&mut app, 120, 24);
    assert!(app.panel_anim_deadline.is_none());
}

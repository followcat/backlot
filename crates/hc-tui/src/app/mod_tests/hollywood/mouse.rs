//! The layer's mouse hits: the lot pill opens the bar, expanded-
//! panel rows act like Enter, card buttons run their action, the
//! input line opens for editing, the wheel scrolls and a press
//! outside leaves the panel open for whatever is under it.
use super::*;

#[tokio::test]
async fn a_click_on_the_lot_pill_expands_the_layer() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    draw_to(&mut app, 120, 24);
    let pill = app.hit_map.lot_pill;
    assert!(!pill.is_empty(), "the pill has a hit");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        pill.x,
        pill.y,
    ));
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
}

#[tokio::test]
async fn a_card_row_click_selects_it() {
    // S1's mouse is select-only: the chip's key still runs `apply`.
    let mut app = app_with_hollywood();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);
    app.hollywood.mode = LayerMode::Expanded;
    draw_to(&mut app, 120, 24);
    let (rect, _) = app
        .hollywood_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Card(id) if id == "c3"))
        .cloned()
        .expect("c3's row hit");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 2,
        rect.y,
    ));
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
    assert!(recorded.lock().unwrap().is_empty(), "no action on click");
    key(&mut app, hc_vt::Key::Char('a'));
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec!["card", "apply", "c3", "--as", "user"]]
    );
}

#[tokio::test]
async fn a_studio_row_click_selects_it() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    app.hollywood.mode = LayerMode::Expanded;
    draw_to(&mut app, 120, 24);
    let (rect, _) = app
        .hollywood_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Studio(id) if id == "acme-shop"))
        .cloned()
        .expect("acme-shop's row hit");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 2,
        rect.y,
    ));
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("acme-shop".into()))
    );
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
}

#[tokio::test]
async fn a_click_on_the_input_line_edits_it() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    app.hollywood.mode = LayerMode::Expanded;
    draw_to(&mut app, 120, 24);
    let input = app.hollywood_hits.input;
    assert!(!input.is_empty());
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        input.x,
        input.y,
    ));
    assert!(app.hollywood.editing);
}

#[tokio::test]
async fn the_wheel_scrolls_the_frames_sideways() {
    let mut app = app_with_hollywood();
    // Narrow enough that the chip strip overflows: the four frames
    // do not all fit, so the window can move sideways.
    app.area = (50, 18);
    app.hollywood.mode = LayerMode::Expanded;
    draw_to(&mut app, 50, 18);
    // Over the chips the wheel scrolls the frame window.
    let frame = app
        .hollywood_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Studio(_)))
        .map(|(rect, _)| *rect)
        .expect("a frame");
    app.on_mouse(mouse(MouseEventKind::ScrollDown, frame.x + 1, frame.y));
    assert!(app.hollywood.frame_scroll > 0, "the frames scrolled");
    app.on_mouse(mouse(MouseEventKind::ScrollUp, frame.x + 1, frame.y));
    assert_eq!(app.hollywood.frame_scroll, 0);
}

#[tokio::test]
async fn a_click_outside_the_panel_goes_under_and_leaves_it_open() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    app.hollywood.mode = LayerMode::Expanded;
    draw_to(&mut app, 120, 24);
    let outside = app.hollywood_hits.area.y + app.hollywood_hits.area.height + 1;
    app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 60, outside));
    assert_eq!(app.hollywood.mode, LayerMode::Expanded, "the panel stayed");
}

#[tokio::test]
async fn an_outside_click_closes_the_show_me_view() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    app.hollywood.mode = LayerMode::Expanded;
    app.hollywood.view = Some(crate::hollywood::CardView {
        title: "c4".into(),
        lines: vec!["one".into(), "two".into()],
        scroll: 0,
    });
    draw_to(&mut app, 120, 24);
    let hits = app
        .hollywood_hits
        .view
        .as_ref()
        .expect("the view's hits")
        .area;
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        hits.x + hits.width + 1,
        hits.y + hits.height + 1,
    ));
    assert!(app.hollywood.view.is_none());
}

#[tokio::test]
async fn a_click_on_the_new_studio_frame_selects_it() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    app.hollywood.mode = LayerMode::Expanded;
    draw_to(&mut app, 120, 24);
    let frame = app
        .hollywood_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::NewStudio))
        .map(|(rect, _)| *rect)
        .expect("the new-studio frame");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        frame.x + 2,
        frame.y,
    ));
    assert_eq!(app.hollywood.selected, Some(Target::NewStudio));
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
}

//! U1 studio mode mouse hits: cast rows focus their person, the
//! `others` row expands in place, a block's top border makes its
//! studio current and a stacked block's line expands it.
use super::studio::two_studio_app;
use super::*;
use crate::app::input::{MouseButton, MouseEventKind, MouseInput};
use crate::panel::PanelMode;

fn mouse(kind: MouseEventKind, col: u16, row: u16) -> MouseInput {
    MouseInput {
        kind,
        col,
        row,
        mods: hc_vt::Mods::NONE,
        sgr_position: None,
    }
}

#[tokio::test]
async fn a_cast_row_click_focuses_the_person() {
    let mut app = two_studio_app();
    app.area = (120, 24);
    draw_to(&mut app, 120, 24);
    let alice = app
        .hit_map
        .agents
        .iter()
        .find(|(_, t)| t == "t4")
        .map(|(rect, _)| *rect)
        .expect("alice's cast row hit");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        alice.x,
        alice.y,
    ));
    assert_eq!(app.focused.as_deref(), Some("t4"));
    assert_eq!(app.studio.current.as_deref(), Some("acme-shop"));
}

#[tokio::test]
async fn the_others_row_expands_and_collapses_in_place() {
    let mut app = two_studio_app();
    // t50/t51 are herdr agents no person seats → `others 2 ▸`.
    app.area = (120, 24);
    draw_to(&mut app, 120, 24);
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains("others 2"), "{text}");
    let others = app.hit_map.cast_others;
    assert!(!others.is_empty(), "the others row hit");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        others.x,
        others.y,
    ));
    assert!(app.studio.others_expanded);
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        others.x,
        others.y,
    ));
    assert!(!app.studio.others_expanded);
}

#[tokio::test]
async fn a_block_top_border_click_makes_that_studio_current() {
    let mut app = two_studio_app();
    app.area = (120, 24);
    app.panel.mode = PanelMode::Expanded;
    draw_to(&mut app, 120, 24);
    // night-owl's top border: a Studio target on the frame row.
    let top = app
        .panel_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, crate::panel::Target::Studio(id) if id == "night-owl"))
        .map(|(rect, _)| *rect)
        .expect("night-owl's frame hit");
    app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), top.x, top.y));
    assert_eq!(app.studio.current.as_deref(), Some("night-owl"));
    assert_eq!(app.focused.as_deref(), Some("t7"));
}

#[tokio::test]
async fn a_stacked_block_click_expands_it() {
    let mut app = two_studio_app();
    app.area = (120, 24);
    app.panel.mode = PanelMode::Expanded;
    // A short body stacks the non-current blocks.
    draw_to(&mut app, 120, 9);
    let stacked = app
        .panel_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, crate::panel::Target::Block(..)))
        .map(|(rect, _)| *rect)
        .expect("a stacked block hit at a short height");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        stacked.x,
        stacked.y,
    ));
    assert_eq!(app.panel.expanded_blocks.len(), 1, "the click expanded it");
}

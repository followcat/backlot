//! U1 studio mode mouse hits: cast rows focus their person, the
//! `others` row expands in place, a block's top border makes its
//! studio current and a stacked block's line expands it.
use super::studio::two_studio_app;
use super::*;
use crate::app::input::{MouseButton, MouseEventKind, MouseInput};
use crate::panel::{PanelMode, Target};

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
    assert_eq!(app.studio.current, Some("acme-shop".into()));
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
    // The block's frame column is the dock's resize handle — the row
    // press lands inside it.
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        top.x + 2,
        top.y,
    ));
    assert_eq!(app.studio.current, Some("night-owl".into()));
    assert_eq!(app.focused.as_deref(), Some("t7"));
}

#[tokio::test]
async fn the_menu_button_opens_the_keybind_launcher() {
    // S5 §2: the sidebar's `menu` is `Ctrl+B ?` — the launcher while
    // studios exist.
    let mut app = two_studio_app();
    app.area = (120, 24);
    draw_to(&mut app, 120, 24);
    let button = app.hit_map.global_launcher;
    assert!(!button.is_empty(), "the `menu` button has a hit");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        button.x,
        button.y,
    ));
    assert!(app.help);
    assert!(app.help_launcher(), "studios — the launcher, not herdr's");
}

#[tokio::test]
async fn the_menu_button_stays_dead_in_plain() {
    let mut app = two_studio_app();
    app.set_plain(true);
    app.area = (120, 24);
    draw_to(&mut app, 120, 24);
    let button = app.hit_map.global_launcher;
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        button.x,
        button.y,
    ));
    assert!(!app.help, "`--plain` keeps herdr's dead hit dead");
}

#[tokio::test]
async fn a_lot_pill_right_click_opens_the_lot_menu() {
    // S5 §2.3: the pill's right-click is the lot menu — the pill is
    // the whole lot, not the current studio.
    let mut app = super::help_launcher::s5_app();
    app.area = (120, 24);
    draw_to(&mut app, 120, 24);
    let pill = app.hit_map.lot_pill;
    assert!(!pill.is_empty(), "the pill has a hit");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Right),
        pill.x,
        pill.y,
    ));
    let menu = app.menu.as_ref().expect("the lot menu opened");
    let crate::chrome::menu::ContextMenuTarget::Studio(inner) = &menu.target else {
        panic!("a studio menu");
    };
    assert!(matches!(inner.kind, crate::studio_menu::MenuKind::Lot));
    let labels: Vec<String> = inner.items().iter().map(|i| i.label.clone()).collect();
    assert_eq!(
        labels,
        [
            "Open Call Sheet",
            "Toggle Studio Dock",
            "New studio…",
            "Hollywood settings…"
        ]
    );
}

#[tokio::test]
async fn the_lot_menu_items_open_their_surfaces() {
    use crate::studio_menu::{StudioAct, StudioMenu};
    let mut app = super::help_launcher::s5_app();
    app.area = (120, 24);
    draw_to(&mut app, 120, 24);
    let menu = StudioMenu::lot();
    app.run_studio_act(&menu, StudioAct::CallSheet);
    assert!(app.hollywood.expanded(), "Open Call Sheet expands the band");
    app.hollywood_collapse();
    app.run_studio_act(&menu, StudioAct::ToggleDock);
    assert!(app.panel.open(), "Toggle Studio Dock opens the dock");
    app.panel_toggle();
    app.run_studio_act(&menu, StudioAct::NewStudio);
    assert!(app.new_studio.is_some(), "New studio… opens S4's dialog");
    app.new_studio = None;
    app.run_studio_act(&menu, StudioAct::HollywoodSettings);
    let settings = app
        .settings
        .as_ref()
        .expect("Hollywood settings… opens Settings");
    assert_eq!(
        settings.section,
        crate::chrome::settings::SettingsSection::Studio,
        "…on the Studio section, where [hollywood] lives"
    );
}

#[tokio::test]
async fn a_studio_header_right_click_opens_the_studio_menu() {
    let mut app = two_studio_app();
    app.area = (120, 24);
    draw_to(&mut app, 120, 24);
    let header = app.hit_map.studio_header;
    assert!(!header.is_empty(), "the header's name has a hit");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Right),
        header.x,
        header.y,
    ));
    let menu = app.menu.as_ref().expect("the studio menu opened");
    assert!(
        matches!(
            menu.target,
            crate::chrome::menu::ContextMenuTarget::Studio(_)
        ),
        "a studio menu"
    );
}

#[tokio::test]
async fn a_cast_row_right_click_opens_its_role_menu() {
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
        MouseEventKind::Down(MouseButton::Right),
        alice.x,
        alice.y,
    ));
    let menu = app.menu.as_ref().expect("the role menu opened");
    let crate::chrome::menu::ContextMenuTarget::Studio(inner) = &menu.target else {
        panic!("a studio menu");
    };
    assert!(
        matches!(inner.kind, crate::studio_menu::MenuKind::Role),
        "alice's role menu"
    );
}

#[tokio::test]
async fn a_click_on_a_row_under_a_divider_selects_the_person() {
    let mut app = two_studio_app();
    app.area = (120, 24);
    app.panel.mode = PanelMode::Expanded;
    let drawn = draw_to(&mut app, 120, 24);
    let buf = drawn.buffer();
    let (rect, target) = app
        .panel_hits
        .rows
        .iter()
        .find(|(_, t)| *t == Target::Person("acme-shop".into(), "actor-alice".into()))
        .cloned()
        .expect("alice's row");
    let above: String = (rect.x..rect.x + rect.width)
        .map(|x| buf[(x, rect.y - 1)].symbol())
        .collect();
    assert!(above.starts_with('├'), "a rule sits above the row: {above}");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 3,
        rect.y,
    ));
    assert_eq!(app.panel.selected, Some(target));
}

#[tokio::test]
async fn a_stacked_block_click_expands_it() {
    let mut app = two_studio_app();
    app.area = (120, 24);
    app.panel.mode = PanelMode::Expanded;
    // A body too short for every block stacks the non-current ones;
    // the first stacked row still fits under the expanded acme-shop.
    draw_to(&mut app, 120, 10);
    let stacked = app
        .panel_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, crate::panel::Target::Block(..)))
        .map(|(rect, _)| *rect)
        .expect("a stacked block hit at a short height");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        stacked.x + 2,
        stacked.y,
    ));
    assert_eq!(app.panel.expanded_blocks.len(), 1, "the click expanded it");
}

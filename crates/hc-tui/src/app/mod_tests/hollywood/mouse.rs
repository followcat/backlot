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
async fn a_card_row_click_acts_like_enter() {
    // S5 §2: a row click selects the row and runs its Enter — a
    // card's Enter is the "Show me" view.
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
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec!["card", "show", "c3", "--as", "user"]]
    );
}

#[tokio::test]
async fn a_studio_row_click_focuses_the_studio() {
    // S5 §2: a studio row's click is its Enter — the dock focuses
    // the studio and the sheet collapses back to the pill.
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
    assert_eq!(app.studio.current, Some("acme-shop".into()));
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
}

#[tokio::test]
async fn a_click_on_the_input_line_edits_it() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    app.hollywood.mode = LayerMode::Expanded;
    app.hollywood.selected = Some(Target::Card("c3".into()));
    draw_to(&mut app, 120, 24);
    let input = app.hollywood_hits.input;
    assert!(!input.is_empty());
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        input.x,
        input.y,
    ));
    assert_eq!(app.hollywood.selected, Some(Target::Input));
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
async fn a_click_outside_the_panel_collapses_it_and_the_pane_owns_keys() {
    // A10.1: the press collapses the bar to the pill and keeps
    // routing — the pane under the click owns the keys, so a typed
    // `a` reaches its backlog.
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    app.hollywood.mode = LayerMode::Expanded;
    draw_to(&mut app, 120, 24);
    let before = app.pane.input_backlog().0;
    let outside = app.hollywood_hits.area.y + app.hollywood_hits.area.height + 1;
    app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 60, outside));
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    key(&mut app, hc_vt::Key::Char('a'));
    assert_eq!(
        app.pane.input_backlog().0,
        before + 1,
        "`a` reached the pane"
    );
}

#[tokio::test]
async fn a_card_apply_chip_runs_apply() {
    // S5 §2: the chip is its own target — `apply` on c3 runs the
    // same `card apply` `a` would.
    let mut app = app_with_hollywood();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);
    app.hollywood.mode = LayerMode::Expanded;
    draw_to(&mut app, 120, 24);
    let (rect, _, _) = app
        .hollywood_hits
        .chips
        .iter()
        .find(|(_, t, key)| matches!(t, Target::Card(id) if id == "c3") && *key == 'a')
        .cloned()
        .expect("c3's apply chip");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 1,
        rect.y,
    ));
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec!["card", "apply", "c3", "--as", "user"]]
    );
}

#[tokio::test]
async fn a_card_cancel_chip_opens_the_reason_dialog() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    app.hollywood.mode = LayerMode::Expanded;
    draw_to(&mut app, 120, 24);
    let (rect, _, _) = app
        .hollywood_hits
        .chips
        .iter()
        .find(|(_, t, key)| matches!(t, Target::Card(id) if id == "c3") && *key == 'x')
        .cloned()
        .expect("c3's cancel chip");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 1,
        rect.y,
    ));
    let crate::studio_dialog::StudioDialog::Reason(_) = app
        .studio_dialog
        .as_ref()
        .expect("the cancel reason dialog")
    else {
        panic!("a reason dialog");
    };
}

#[tokio::test]
async fn a_prompts_allow_chip_decides_it() {
    let mut app = super::super::help_launcher::s5_app();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);
    app.hollywood.mode = LayerMode::Expanded;
    draw_to(&mut app, 120, 24);
    let (rect, _, _) = app
        .hollywood_hits
        .chips
        .iter()
        .find(|(_, t, key)| matches!(t, Target::Prompt(..)) && *key == 'y')
        .cloned()
        .expect("alice's allow chip");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 1,
        rect.y,
    ));
    let argv = recorded.lock().unwrap();
    assert_eq!(argv.len(), 1, "{argv:?}");
    let line = argv[0].join(" ");
    assert!(line.contains("approve") && line.contains("allow"), "{line}");
}

#[tokio::test]
async fn a_stages_start_chip_starts_the_stage() {
    // Drop the stage terminal: the `stage stopped` row carries
    // the `s start` chip.
    let mut app = super::super::help_launcher::s5_app();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    let terminals: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .filter(|t| t.name.as_deref() != Some("backlot-stage"))
        .cloned()
        .collect();
    app.view.inventory_mut().replace(2, terminals);
    app.area = (120, 24);
    app.hollywood.mode = LayerMode::Expanded;
    draw_to(&mut app, 120, 24);
    let (rect, _, _) = app
        .hollywood_hits
        .chips
        .iter()
        .find(|(_, t, key)| matches!(t, Target::Stage(..)) && *key == 's')
        .cloned()
        .expect("the stage's start chip");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 1,
        rect.y,
    ));
    let argv = recorded.lock().unwrap();
    assert_eq!(argv.len(), 1, "{argv:?}");
    assert!(argv[0].join(" ").contains("stage"), "{}", argv[0].join(" "));
}

#[tokio::test]
async fn a_ready_rows_finish_chip_runs_finish() {
    let mut app = super::super::help_launcher::s5_app();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);
    app.hollywood.mode = LayerMode::Expanded;
    draw_to(&mut app, 120, 24);
    let (rect, _, _) = app
        .hollywood_hits
        .chips
        .iter()
        .find(|(_, t, key)| matches!(t, Target::Ready(..)) && *key == 'f')
        .cloned()
        .expect("beta-tools' finish chip");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 1,
        rect.y,
    ));
    // `f` arms the row's inline confirm — `y` runs `studio finish`.
    assert!(
        matches!(
            app.hollywood.confirm,
            Some(crate::hollywood::Confirm::Finish(_))
        ),
        "the finish confirm armed"
    );
    key(&mut app, hc_vt::Key::Char('y'));
    let argv = recorded.lock().unwrap();
    assert_eq!(argv.len(), 1, "{argv:?}");
    let line = argv[0].join(" ");
    assert!(line.contains("finish"), "{line}");
    assert!(line.contains("beta-tools"), "{line}");
}

#[tokio::test]
async fn a_title_right_click_opens_the_studio_menu() {
    let mut app = super::super::help_launcher::s5_app();
    app.area = (120, 24);
    app.hollywood.mode = LayerMode::Expanded;
    draw_to(&mut app, 120, 24);
    let title = app.hollywood_hits.title;
    assert!(!title.is_empty(), "the title row's hit");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Right),
        title.x + 2,
        title.y,
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
async fn a_row_right_click_selects_it_and_opens_its_menu() {
    let mut app = app_with_hollywood();
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
        MouseEventKind::Down(MouseButton::Right),
        rect.x + 2,
        rect.y,
    ));
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
    let menu = app.menu.as_ref().expect("the card's menu opened");
    assert!(
        matches!(
            menu.target,
            crate::chrome::menu::ContextMenuTarget::Studio(_)
        ),
        "a studio menu"
    );
}

#[tokio::test]
async fn an_outside_click_closes_the_show_me_view() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    app.hollywood.mode = LayerMode::Expanded;
    app.hollywood.view = Some(crate::hollywood::CardView {
        card: Some("c4".into()),
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
async fn a_click_on_the_new_studio_frame_opens_the_dialog() {
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
    assert!(app.new_studio.is_some(), "the native dialog opened");
    assert!(app.popup_pending.is_none(), "the popup wizard is retired");
}

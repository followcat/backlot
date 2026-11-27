//! The layer's mouse hits: the lot pill opens the bar, expanded-
//! panel rows act like Enter, card buttons run their action, the
//! input line opens for editing, the wheel scrolls and a press
//! outside leaves the panel open for whatever is under it.
use super::*;
use crate::studio::StudioKey;

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
    // Clicking a card row selects & focuses it; clicking its `show ↵` chip
    // runs `card show`.
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
    assert!(recorded.lock().unwrap().is_empty());
    draw_to(&mut app, 120, 24);
    let (chip_rect, _, _) = app
        .hollywood_hits
        .chips
        .iter()
        .find(|(_, t, k)| matches!(t, Target::Card(id) if id == "c3") && *k == '\r')
        .cloned()
        .expect("c3 show chip");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        chip_rect.x + 1,
        chip_rect.y,
    ));
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
    app.hollywood.selected = Some(Target::Card("c3".into()));
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
    app.hollywood.selected = Some(Target::Card("c3".into()));
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
    app.hollywood.selected = app
        .hollywood_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Prompt(..)))
        .map(|(_, t)| t.clone());
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
    app.hollywood.selected = app
        .hollywood_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Stage(..)))
        .map(|(_, t)| t.clone());
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
    app.hollywood.selected = app
        .hollywood_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Ready(..)))
        .map(|(_, t)| t.clone());
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
        ..crate::hollywood::CardView::default()
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

#[tokio::test]
async fn clicking_row_two_under_allow_runs_allow_and_row_text_stays_on_sheet() {
    let mut app = app_with_hollywood();
    let mut value = summary_json();
    value["hollywood"]["work"] = "blocked".into();
    value["hollywood"]["attention_user"] = true.into();
    value["hollywood"]["attention_detail"] = "backlot --help".into();
    value["hollywood"]["cards"] = serde_json::json!([]);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 44);
    app.hollywood.mode = LayerMode::Expanded;
    app.hollywood.selected = Some(Target::Hollywood);
    draw_to(&mut app, 120, 44);

    let (row_rect, _) = app
        .hollywood_hits
        .rows
        .iter()
        .find(|(_, t)| *t == Target::Hollywood)
        .cloned()
        .expect("HOLLYWOOD row hit");
    assert_eq!(row_rect.height, 2, "detail row expands height to 2");

    // Clicking row text (not `[open ↵]`) selects it without leaving the Call Sheet.
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        row_rect.x + 4,
        row_rect.y,
    ));
    assert_eq!(app.hollywood.selected, Some(Target::Hollywood));
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    assert_ne!(app.focused.as_deref(), Some("t10"));
    draw_to(&mut app, 120, 24);

    // Clicking the 2nd line directly beneath `[allow y]` hits the chip, not `open`.
    let (chip_rect, _, _) = app
        .hollywood_hits
        .chips
        .iter()
        .find(|(_, t, key)| *t == Target::Hollywood && *key == 'y')
        .cloned()
        .expect("HOLLYWOOD allow chip");
    assert_eq!(chip_rect.height, 2);
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        chip_rect.x + 1,
        chip_rect.y + 1,
    ));
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec![
            "approve",
            "--person",
            "hollywood",
            "--decision",
            "allow",
            "--as",
            "user"
        ]]
    );
}

#[tokio::test]
async fn lot_pill_segments_jump_to_clicked_bucket_and_alt_y_lands_on_blocking() {
    let mut app = app_with_hollywood();
    let mut value = summary_json();
    value["studios"][0]["roles"][1]["attention"] = "needs permission".into();
    value["studios"][0]["roles"][1]["attention_seq"] = serde_json::json!(7);
    value["studios"][0]["roles"][1]["attention_user"] = true.into();
    value["studios"][0]["roles"][0]["work"] = "waiting".into();
    value["studios"][0]["roles"][0]["asks"] = "ship now?".into();
    value["studios"][0]["ready_to_finish"] = true.into();
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.area = (160, 44);
    draw_to(&mut app, 160, 44);

    assert_eq!(app.studio.lot.counts.blocking, 1);
    assert_eq!(app.studio.lot.counts.asks, 1);
    assert_eq!(app.studio.lot.counts.cards, 2);
    assert_eq!(app.studio.lot.counts.ready, 1);

    let click_bucket = |app: &mut App, bucket: crate::chrome::lot::PillAction| {
        app.hollywood.mode = LayerMode::Collapsed;
        draw_to(app, 160, 44);
        let rect = app
            .hit_map
            .lot_pill_segments
            .iter()
            .find(|(_, a)| *a == bucket)
            .map(|(r, _)| *r)
            .unwrap_or_else(|| panic!("missing pill segment {bucket:?}"));
        app.on_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            rect.x,
            rect.y,
        ));
    };

    // `!1` -> first blocking item (`Resting`), `?1` -> asks role, `2c` -> first card, `✓1` -> ready.
    click_bucket(&mut app, crate::chrome::lot::PillAction::Asks);
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Role(
            StudioKey::local("acme-shop"),
            "producer".into()
        ))
    );
    assert!(!app.hollywood.editing);

    click_bucket(&mut app, crate::chrome::lot::PillAction::Cards);
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
    assert!(!app.hollywood.editing);

    click_bucket(&mut app, crate::chrome::lot::PillAction::Ready);
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Ready(StudioKey::local("acme-shop")))
    );
    assert!(!app.hollywood.editing);

    click_bucket(&mut app, crate::chrome::lot::PillAction::Blocking);
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Prompt(
            StudioKey::local("acme-shop"),
            "actor-alice".into()
        ))
    );
    assert!(!app.hollywood.editing);

    // Smart `Alt+Y` with `blocking > 0` lands on the 1st blocking item (`Resting`).
    app.hollywood.mode = LayerMode::Collapsed;
    alt_y(&mut app);
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Prompt(
            StudioKey::local("acme-shop"),
            "actor-alice".into()
        ))
    );
    assert!(!app.hollywood.editing);
}

#[tokio::test]
async fn clicking_recast_card_rows_and_chips_on_ready_studio_targets_each_card() {
    let mut app = app_with_hollywood();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    let mut doc = summary_json();
    doc["studios"][0]["ready_to_finish"] = serde_json::json!(true);
    doc["studios"][0]["tasks_done"] = serde_json::json!(12);
    doc["studios"][0]["tasks_total"] = serde_json::json!(13);
    doc["hollywood"]["cards"] = serde_json::json!([
        { "id": "c1372", "studio": "acme-shop", "action": "recast producer → agy/gemini-3-pro/high", "why": "quota" },
        { "id": "c1373", "studio": "acme-shop", "action": "recast director → agy/gemini-3-pro/high", "why": "quota" },
        { "id": "c1374", "studio": "acme-shop", "action": "recast writer → agy/gemini-3-pro/high", "why": "quota" }
    ]);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.area = (200, 17);
    app.hollywood.mode = LayerMode::Expanded;
    draw_to(&mut app, 200, 17);

    let (c1373_rect, _) = app
        .hollywood_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Card(id) if id == "c1373"))
        .cloned()
        .expect("c1373 hit");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        c1373_rect.x + 4,
        c1373_rect.y,
    ));
    assert_eq!(app.hollywood.selected, Some(Target::Card("c1373".into())));
    assert!(recorded.lock().unwrap().is_empty());
    draw_to(&mut app, 200, 17);
    let (show_rect, _, _) = app
        .hollywood_hits
        .chips
        .iter()
        .find(|(_, t, k)| matches!(t, Target::Card(id) if id == "c1373") && *k == '\r')
        .cloned()
        .expect("c1373 show chip");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        show_rect.x + 1,
        show_rect.y,
    ));
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec!["card", "show", "c1373", "--as", "user"]]
    );

    // Close the CardView and click the `apply a` chip on `c1374`.
    app.hollywood.view = None;
    app.hollywood.selected = Some(Target::Card("c1374".into()));
    draw_to(&mut app, 200, 17);
    let (chip_rect, _, _) = app
        .hollywood_hits
        .chips
        .iter()
        .find(|(_, t, k)| matches!(t, Target::Card(id) if id == "c1374") && *k == 'a')
        .cloned()
        .expect("c1374 apply chip");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        chip_rect.x + 1,
        chip_rect.y,
    ));
    assert_eq!(
        recorded.lock().unwrap().last().unwrap().as_slice(),
        &["card", "apply", "c1374", "--as", "user"]
    );
}

#[tokio::test]
async fn rail_crossbar_and_sill_clicks_keep_exact_hit_rect_parity() {
    let mut app = app_with_hollywood();
    app.area = (120, 28);
    alt_y(&mut app);
    draw_to(&mut app, 120, 28);

    // 1. Clicking col 0 (`▌`/`║`) or col `width - 1` (`║`) on a wait row selects it identically to clicking inner margin.
    let (card_rect, _) = app
        .hollywood_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Card(id) if id == "c4"))
        .cloned()
        .expect("c4 row hit");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        card_rect.x,
        card_rect.y,
    ));
    assert_eq!(app.hollywood.selected, Some(Target::Card("c4".into())));
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);

    // Right rail (`width - 1`) on `c3` also selects `c3`.
    let (c3_rect, _) = app
        .hollywood_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Card(id) if id == "c3"))
        .cloned()
        .expect("c3 row hit");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        c3_rect.right() - 1,
        c3_rect.y,
    ));
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));

    // 2. Compose crossbar inner field (`field.x`) enters compose editing; `hits.input` spans `[MARGIN .. width - MARGIN)`.
    let input_rect = app.hollywood_hits.input;
    assert_eq!(input_rect.x, app.hollywood_hits.area.x + 2);
    assert_eq!(input_rect.right(), app.hollywood_hits.area.right() - 2);
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        input_rect.x,
        input_rect.y,
    ));
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert!(app.hollywood.editing);

    // 3. Clicking the marquee sill (`area.bottom() - 1`) stays inside the band (does not collapse);
    // clicking immediately below it (`area.bottom()`) collapses.
    let sill_y = app.hollywood_hits.area.bottom() - 1;
    let mid_x = app.hollywood_hits.area.x + app.hollywood_hits.area.width / 2;
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        mid_x,
        sill_y,
    ));
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);

    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        mid_x,
        sill_y + 1,
    ));
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
}

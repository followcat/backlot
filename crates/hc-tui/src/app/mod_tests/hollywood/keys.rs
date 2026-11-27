//! The layer's keys at App level: the configured direct binding and
//! the `prefix+` chord, expanded-panel ownership (movement, actions,
//! the input line, `x` and `Esc`) and the strip's pass-through.
use super::*;

#[tokio::test]
async fn a_configured_direct_key_replaces_alt_y() {
    let mut app = app_with_doc();
    app.set_hollywood(
        Ok(Some(cfg(Some("ctrl+h")))),
        std::path::Path::new("config.toml"),
    );
    assert_eq!(
        app.hollywood.mode,
        LayerMode::Collapsed,
        "the entry state — the pill is the bar's face (S0)"
    );
    app.area = (120, 24);
    // Alt+Y is no longer the layer's — the strip stays a strip.
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    key_mods(&mut app, hc_vt::Key::Char('h'), hc_vt::Mods::CTRL);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
}

#[tokio::test]
async fn a_configured_chord_toggles_through_the_prefix() {
    let mut app = app_with_doc();
    app.set_hollywood(
        Ok(Some(cfg(Some("prefix+h")))),
        std::path::Path::new("config.toml"),
    );
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    app.area = (120, 24);
    // The direct slot `[hollywood] key` never claimed keeps its
    // `alt+y` default — only `[keys] call_sheet = ""` frees it.
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    key(&mut app, hc_vt::Key::Escape);
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('h'));
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    // The same chord collapses it back.
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('h'));
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
}

#[tokio::test]
async fn a_refused_key_falls_back_to_alt_y_with_a_notice() {
    let mut app = app_with_doc();
    // `alt+pageup` is `previous_studio` — refused, never echoed.
    app.set_hollywood(
        Ok(Some(cfg(Some("alt+pageup")))),
        std::path::Path::new("config.toml"),
    );
    assert!(app.notice.is_some(), "a refused key notices");
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains("previous_studio"), "{text}");
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    app.area = (120, 24);
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
}

#[tokio::test]
async fn expanded_keys_walk_the_frames_and_the_band() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    // S1: Alt+Y lands in Compose (`Section::Hollywood`); screen-order Tab
    // moves `Hollywood -> Studios -> Waiting (c3) -> Hollywood`.
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert!(app.hollywood.editing);
    key(&mut app, hc_vt::Key::Tab);
    assert!(!app.hollywood.editing);
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("acme-shop".into()))
    );
    // → / l move within STUDIOS frames and clamp at `+ new studio`.
    key(&mut app, hc_vt::Key::Char('l'));
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("beta-tools".into()))
    );
    key(&mut app, hc_vt::Key::Char('l'));
    assert_eq!(app.hollywood.selected, Some(Target::NewStudio));
    key(&mut app, hc_vt::Key::Char('l'));
    assert_eq!(app.hollywood.selected, Some(Target::NewStudio));
    // ↓ at the bottom of STUDIOS stays on `+ new studio`; ← comes back.
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.hollywood.selected, Some(Target::NewStudio));
    key(&mut app, hc_vt::Key::Char('h'));
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("beta-tools".into()))
    );

    // Tab advances `Studios -> Waiting (c3)`, where `←`/`→` step action chips.
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
    key(&mut app, hc_vt::Key::Char('h'));
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
    assert_eq!(app.hollywood.chip_index, Some(1));
    // At top of WAITING, ↑ clamps; ↓ walks down through Headline & Input to STUDIOS.
    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
    key(&mut app, hc_vt::Key::Char('j'));
    assert_eq!(app.hollywood.selected, Some(Target::Card("c4".into())));
    key(&mut app, hc_vt::Key::Char('j'));
    assert_eq!(app.hollywood.selected, Some(Target::Headline));
    key(&mut app, hc_vt::Key::Char('j'));
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert!(!app.hollywood.editing);
    key(&mut app, hc_vt::Key::Char('j'));
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("beta-tools".into()))
    );

    // Screen-order ring: `Studios -> Waiting -> Hollywood (Compose) -> Studios`.
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert!(app.hollywood.editing);
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("beta-tools".into()))
    );
    // Nothing reached the focused program.
    assert_eq!(app.pane.input_backlog().0, 0, "expanded keys leaked");
}

#[tokio::test]
async fn tab_switches_sections_from_compose_while_keeping_draft_and_section_memory() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    alt_y(&mut app);
    key(&mut app, hc_vt::Key::Char('h'));
    key(&mut app, hc_vt::Key::Char('i'));
    assert!(app.hollywood.editing);
    assert_eq!(app.hollywood.input.as_str(), "hi");

    // Screen-order `Tab` from `Compose`: `Studios -> Waiting (c3) -> Compose (Input)`,
    // keeping draft intact; `Shift+Tab` reverses `Compose -> Waiting -> Studios -> Compose`.
    key(&mut app, hc_vt::Key::Tab);
    assert!(!app.hollywood.editing);
    assert_eq!(app.hollywood.input.as_str(), "hi");
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("acme-shop".into()))
    );
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert!(app.hollywood.editing);

    // `Shift+Tab` (`BackTab`) reverses: `Compose -> Waiting (c3) -> Studios`.
    key_mods(&mut app, hc_vt::Key::BackTab, hc_vt::Mods::SHIFT);
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
    key_mods(&mut app, hc_vt::Key::BackTab, hc_vt::Mods::SHIFT);
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("acme-shop".into()))
    );
    key_mods(&mut app, hc_vt::Key::BackTab, hc_vt::Mods::SHIFT);
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert!(app.hollywood.editing);
    assert_eq!(app.hollywood.input.as_str(), "hi");
}

#[tokio::test]
async fn lower_section_arrows_walk_hollywood_ask_live_and_input_without_looping() {
    let mut app = app_with_hollywood();
    let mut value = summary_json();
    value["hollywood"]["cards"] = serde_json::json!([]);
    value["hollywood"]["exchange"] =
        serde_json::json!([{ "seq": 8, "who": "user", "text": "status?" }]);
    value["hollywood"]["work"] = "blocked".into();
    value["hollywood"]["attention_user"] = true.into();
    value["hollywood"]["attention_detail"] = "backlot --help".into();
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.area = (120, 30);
    alt_y(&mut app);
    // Smart Alt+Y lands straight on the blocking `Wait::Hollywood` (`Resting`).
    assert!(!app.hollywood.editing);
    assert_eq!(app.hollywood.selected, Some(Target::Hollywood));
    // ↓ crosses zone boundaries smoothly (`Waiting -> Hollywood -> Studios`) without looping.
    let mut visited = vec![app.hollywood.selected.clone().unwrap()];
    for _ in 0..8 {
        key(&mut app, hc_vt::Key::ArrowDown);
        let sel = app.hollywood.selected.clone().unwrap();
        if visited.last() != Some(&sel) {
            visited.push(sel);
        }
    }
    assert_eq!(
        visited,
        vec![
            Target::Hollywood,
            Target::Talk(8),
            Target::HollywoodLive,
            Target::Input,
            Target::Studio("acme-shop".into()),
        ]
    );
    // ↑ walks back up across Input -> Live -> Talk -> Headline -> Waiting without wrapping past top.
    for _ in 0..10 {
        key(&mut app, hc_vt::Key::ArrowUp);
    }
    assert_eq!(app.hollywood.selected, Some(Target::Hollywood));
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.hollywood.selected, Some(Target::Talk(8)));
}

#[tokio::test]
async fn esc_collapses_and_x_is_the_card_cancel() {
    let mut app = app_with_hollywood();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);
    alt_y(&mut app);
    key(&mut app, hc_vt::Key::Escape);
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    // `x` is no hide now — on a selected card it is its cancel.
    alt_y(&mut app);
    app.hollywood_select(Target::Card("c3".into()));
    key(&mut app, hc_vt::Key::Char('x'));
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    key(&mut app, hc_vt::Key::Enter);
    assert!(recorded.lock().unwrap()[0][..3] == ["card", "cancel", "c3"]);
}

#[tokio::test]
async fn a_collapsed_strip_consumes_no_key() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    let before = app.pane.input_backlog().0;
    for k in [
        hc_vt::Key::Char('j'),
        hc_vt::Key::Char('a'),
        hc_vt::Key::Enter,
    ] {
        key(&mut app, k);
    }
    assert!(app.pane.input_backlog().0 > before, "the strip ate keys");
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
}

#[tokio::test]
async fn the_input_line_edits_then_esc_leaves() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    alt_y(&mut app);
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert!(app.hollywood.editing);
    // Typing lands in the editor, not the pane.
    key(&mut app, hc_vt::Key::Char('h'));
    key(&mut app, hc_vt::Key::Char('i'));
    assert_eq!(app.hollywood.input.as_str(), "hi");
    assert_eq!(app.pane.input_backlog().0, 0);
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.hollywood.editing);
}

#[tokio::test]
async fn i_from_a_non_input_row_moves_selection_to_input() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    alt_y(&mut app);
    app.hollywood_select(Target::Studio("acme-shop".into()));
    assert!(!app.hollywood.editing);

    key(&mut app, hc_vt::Key::Char('i'));
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert_eq!(
        app.hollywood.editing,
        app.hollywood.selected == Some(Target::Input),
        "single-focus invariant: editing matches Input selection"
    );
}

#[tokio::test]
async fn the_panel_chord_still_toggles_while_the_layer_is_expanded() {
    let mut app = app_with_hollywood();
    app.area = (200, 24);
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    // The studio panel keeps `prefix+y`.
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(app.panel.mode, crate::panel::PanelMode::Expanded);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
}

#[tokio::test]
async fn n_opens_the_new_studio_wizard_and_collapses_the_bar() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    key(&mut app, hc_vt::Key::Escape);
    key(&mut app, hc_vt::Key::Char('n'));
    assert_eq!(
        app.hollywood.mode,
        LayerMode::Collapsed,
        "the popup is never under the bar"
    );
    assert!(app.new_studio.is_some(), "the native dialog opened");
    assert!(app.popup_pending.is_none(), "the popup wizard is retired");
}

#[tokio::test]
async fn enter_on_the_new_studio_frame_opens_the_wizard() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    alt_y(&mut app);
    app.hollywood_select(Target::NewStudio);
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    assert!(app.new_studio.is_some(), "the native dialog opened");
    assert!(app.popup_pending.is_none(), "the popup wizard is retired");
}

#[tokio::test]
async fn esc_on_a_sheet_menu_keeps_the_bar_owning_keys() {
    // S3 round 2: closing the menu returns the keys to the surface
    // it was opened from — the expanded bar. `m` reopens the row's
    // menu and no byte reaches the focused terminal.
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    alt_y(&mut app);
    app.hollywood_select(Target::Card("c3".into()));
    let before = app.pane.input_backlog().0;
    key(&mut app, hc_vt::Key::Char('m'));
    let studio_menu = matches!(
        app.menu.as_ref().map(|m| &m.target),
        Some(crate::chrome::menu::ContextMenuTarget::Studio(_))
    );
    assert!(studio_menu, "`m` opened the card's menu");
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.menu.is_none(), "Esc closed the menu");
    assert!(app.hollywood.expanded(), "the bar still owns the keys");
    key(&mut app, hc_vt::Key::Char('m'));
    let studio_menu = matches!(
        app.menu.as_ref().map(|m| &m.target),
        Some(crate::chrome::menu::ContextMenuTarget::Studio(_))
    );
    assert!(studio_menu, "`m` opened the menu again");
    assert_eq!(
        app.pane.input_backlog().0,
        before,
        "no byte reached the pane"
    );
}

#[tokio::test]
async fn question_mark_opens_the_help_launcher_over_the_band() {
    let mut app = app_with_doc();
    app.set_hollywood(Ok(Some(cfg(None))), std::path::Path::new("config.toml"));
    app.area = (140, 40);
    alt_y(&mut app);
    key(&mut app, hc_vt::Key::Escape);
    key(&mut app, hc_vt::Key::Char('?'));
    assert!(app.help, "`?` reached the help");
    let text = screen_lines(&draw_to(&mut app, 140, 40)).join("\n");
    assert!(text.contains("Actions & Keybinds"), "{text}");
}

#[tokio::test]
async fn a_kitty_shifted_question_mark_opens_the_help_too() {
    // A kitty-reporting host (REPORT_ALTERNATE_KEYS) sends `?` as the
    // shifted key: the band must read it as the bare `?`, not drop it.
    let mut app = app_with_doc();
    app.set_hollywood(Ok(Some(cfg(None))), std::path::Path::new("config.toml"));
    app.area = (140, 40);
    app.host_reports_all_keys = true;
    alt_y(&mut app);
    key(&mut app, hc_vt::Key::Escape);
    key_mods(&mut app, hc_vt::Key::Char('?'), hc_vt::Mods::SHIFT);
    assert!(app.help, "Shift+? is `?` on a kitty host");
    let text = screen_lines(&draw_to(&mut app, 140, 40)).join("\n");
    assert!(text.contains("Actions & Keybinds"), "{text}");
}

/// R3-B2's paste/IME routing: a composing `›` takes `Paste`/`Text`
/// as editor input; a resting sheet still eats them — the band owns
/// the event whether or not it is a key it maps.
#[tokio::test]
async fn paste_and_ime_land_only_in_a_composing_line() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    alt_y(&mut app);
    assert!(app.hollywood.editing);
    app.on_input(UiEvent::Paste(b"via paste".to_vec()), 120);
    assert_eq!(app.hollywood.input.as_str(), "via paste");
    app.on_input(UiEvent::Text(" + ime".into()), 120);
    assert_eq!(app.hollywood.input.as_str(), "via paste + ime");
    // Resting: the same events are eaten — the parked draft and the
    // pane's input queue are both untouched.
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.hollywood.editing);
    app.on_input(UiEvent::Paste(b"rest".to_vec()), 120);
    app.on_input(UiEvent::Text("text".into()), 120);
    assert_eq!(app.hollywood.input.as_str(), "via paste + ime");
    assert_eq!(app.pane.input_backlog().0, 0, "nothing leaked");
}

/// R3-M11's Home/End: `Resting` they are the first frame / the
/// band's last row; `Compose` hands them to the editor's cursor.
#[tokio::test]
async fn home_end_move_the_sheet_or_the_lines_cursor() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    alt_y(&mut app);
    key(&mut app, hc_vt::Key::Escape);
    // Resting: End lands on the band's last row (the `›` line),
    // Home on the first frame — never arms Compose.
    key(&mut app, hc_vt::Key::End);
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert!(!app.hollywood.editing);
    key(&mut app, hc_vt::Key::Home);
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("acme-shop".into()))
    );
    assert!(!app.hollywood.editing);
    // Compose: Home/End are the editor's own — cursor, not rows.
    key(&mut app, hc_vt::Key::Char('i'));
    for ch in "abc".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Home);
    key(&mut app, hc_vt::Key::Char('x'));
    key(&mut app, hc_vt::Key::End);
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(app.hollywood.input.as_str(), "xabcy");
    assert_eq!(app.hollywood.selected, Some(Target::Input));
}

#[tokio::test]
async fn six_studios_recast_cards_keep_selection_tab_cycle_and_waiting_scroll() {
    let mut app = app_with_hollywood();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    let mut doc = summary_json();
    doc["studios"] = serde_json::json!([
        {
            "id": "acme-shop", "title": "Acme shop", "state": "live",
            "requirement_version": 2, "mode": "standard", "open_reviews": 0,
            "tasks_done": 12, "tasks_total": 13, "ready_to_finish": true, "waiting": 0,
            "roles": [{ "role": "producer", "name": "producer", "terminal": "t1", "state": "live", "work": "waiting" }]
        },
        {
            "id": "beta-api", "title": "Beta api", "state": "live",
            "requirement_version": 1, "mode": "standard", "open_reviews": 0,
            "tasks_done": 13, "tasks_total": 13, "ready_to_finish": true, "waiting": 0,
            "roles": [{ "role": "producer", "name": "producer", "terminal": "t2", "state": "live", "work": "waiting" }]
        },
        {
            "id": "gamma-web", "title": "Gamma web", "state": "live",
            "requirement_version": 1, "mode": "standard", "open_reviews": 0,
            "tasks_done": 1, "tasks_total": 4, "waiting": 0,
            "roles": []
        },
        {
            "id": "delta-ops", "title": "Delta ops", "state": "live",
            "requirement_version": 1, "mode": "standard", "open_reviews": 0,
            "tasks_done": 2, "tasks_total": 4, "waiting": 0,
            "roles": []
        },
        {
            "id": "eps-ui", "title": "Eps ui", "state": "live",
            "requirement_version": 1, "mode": "standard", "open_reviews": 0,
            "tasks_done": 0, "tasks_total": 2, "waiting": 0,
            "roles": []
        },
        {
            "id": "zeta-jobs", "title": "Zeta jobs", "state": "live",
            "requirement_version": 1, "mode": "standard", "open_reviews": 0,
            "tasks_done": 1, "tasks_total": 2, "waiting": 0,
            "roles": []
        }
    ]);
    doc["hollywood"]["cards"] = serde_json::json!([
        { "id": "c1372", "studio": "acme-shop", "action": "recast producer → agy/gemini-3-pro/high", "why": "quota" },
        { "id": "c1373", "studio": "acme-shop", "action": "recast director → agy/gemini-3-pro/high", "why": "quota" },
        { "id": "c1374", "studio": "acme-shop", "action": "recast writer → agy/gemini-3-pro/high", "why": "quota" }
    ]);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc.clone(),
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.area = (200, 17);
    alt_y(&mut app);
    draw_to(&mut app, 200, 17);

    // Tab cycles Hollywood -> Studios -> Waiting (c1372) -> Hollywood.
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    key(&mut app, hc_vt::Key::Tab);
    assert!(matches!(app.hollywood.selected, Some(Target::Studio(_))));
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(app.hollywood.selected, Some(Target::Card("c1372".into())));
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    key(&mut app, hc_vt::Key::BackTab);
    assert_eq!(app.hollywood.selected, Some(Target::Card("c1372".into())));

    // ↓ walks every card and both READY rows in order; ↑ walks back.
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.hollywood.selected, Some(Target::Card("c1373".into())));
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.hollywood.selected, Some(Target::Card("c1374".into())));
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Ready(crate::studio::StudioKey::local("acme-shop")))
    );
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Ready(crate::studio::StudioKey::local("beta-api")))
    );
    key(&mut app, hc_vt::Key::ArrowUp);
    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(app.hollywood.selected, Some(Target::Card("c1374".into())));

    // Applying c1374 directly from the Call Sheet targets c1374.
    key(&mut app, hc_vt::Key::Char('a'));
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec!["card", "apply", "c1374", "--as", "user"]]
    );

    // Waiting scroll: add enough cards (13 waits total) to overflow cap=12 at 100x17
    // and verify PageDown/PageUp + ↓ scroll.
    doc["hollywood"]["cards"] = serde_json::json!([
        { "id": "c1372", "studio": "acme-shop", "action": "recast producer → agy/gemini-3-pro/high", "why": "quota" },
        { "id": "c1373", "studio": "acme-shop", "action": "recast director → agy/gemini-3-pro/high", "why": "quota" },
        { "id": "c1374", "studio": "acme-shop", "action": "recast writer → agy/gemini-3-pro/high", "why": "quota" },
        { "id": "c1375", "studio": "gamma-web", "action": "pause t1", "why": "loop" },
        { "id": "c1376", "studio": "delta-ops", "action": "pause t2", "why": "loop" },
        { "id": "c1377", "studio": "eps-ui", "action": "pause t3", "why": "loop" },
        { "id": "c1378", "studio": "zeta-jobs", "action": "pause t4", "why": "loop" },
        { "id": "c1379", "studio": "gamma-web", "action": "pause t5", "why": "loop" },
        { "id": "c1380", "studio": "delta-ops", "action": "pause t6", "why": "loop" },
        { "id": "c1381", "studio": "eps-ui", "action": "pause t7", "why": "loop" },
        { "id": "c1382", "studio": "zeta-jobs", "action": "pause t8", "why": "loop" }
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
    app.area = (100, 17);
    app.hollywood_select(Target::Card("c1372".into()));
    draw_to(&mut app, 100, 17);
    let shown = app.hollywood_hits.viewport_rows;
    assert!(shown < 13, "expected overflow at 100x17, shown={shown}");
    key(&mut app, hc_vt::Key::PageDown);
    assert!(app.hollywood.scroll > 0, "PageDown scrolls waiting list");
    key(&mut app, hc_vt::Key::PageUp);
    assert_eq!(app.hollywood.scroll, 0, "PageUp scrolls back to top");
    for _ in 0..12 {
        key(&mut app, hc_vt::Key::ArrowDown);
        draw_to(&mut app, 100, 17);
    }
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Ready(crate::studio::StudioKey::local("beta-api")))
    );
    assert!(
        app.hollywood.scroll > 0,
        "moving to bottom waiting row scrolls window"
    );
}

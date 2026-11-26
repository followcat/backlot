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
    // S1: Alt+Y lands in Compose; first `Tab` lands on first waiting item (`c3`) Resting.
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert!(app.hollywood.editing);
    key(&mut app, hc_vt::Key::Tab);
    assert!(!app.hollywood.editing);
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
    // In `Lower`, ↑/↓ walk strictly within the band and clamp at the
    // top (Headline) without jumping into the studio frames.
    key(&mut app, hc_vt::Key::Char('k'));
    assert_eq!(app.hollywood.selected, Some(Target::Headline));
    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Headline),
        "up at the top of Lower stays in Lower"
    );
    key(&mut app, hc_vt::Key::Char('j'));
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
    // ←/→ in Lower step action chips on the selected row rather than
    // jumping back up to the studio frames (`show ↵` is index 2 -> `apply a` is 0).
    key(&mut app, hc_vt::Key::Char('h'));
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
    assert_eq!(app.hollywood.chip_index, Some(1));

    // `Tab` switches to the upper studio-frame section.
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("acme-shop".into()))
    );
    // → / l move within the frames and clamp at `+ new studio`.
    key(&mut app, hc_vt::Key::Char('l'));
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("beta-tools".into()))
    );
    key(&mut app, hc_vt::Key::Char('l'));
    assert_eq!(app.hollywood.selected, Some(Target::NewStudio));
    key(&mut app, hc_vt::Key::Char('l'));
    assert_eq!(app.hollywood.selected, Some(Target::NewStudio));
    // ↓ in `Upper` stays in `Upper` rather than dropping into `Lower`.
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.hollywood.selected, Some(Target::NewStudio));
    // ← / h come back.
    key(&mut app, hc_vt::Key::Char('h'));
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("beta-tools".into()))
    );

    // 3-zone ring from `Upper`: `Tab` -> `Compose` -> `Waiting` (`c3`) -> `Upper` (`beta-tools`).
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert!(app.hollywood.editing);
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
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

    // `Tab` out of `Compose` rests editing, keeps the draft, and hits first Wait (`c3`),
    // then `Studios` (`Upper`), then back into `Compose`.
    key(&mut app, hc_vt::Key::Tab);
    assert!(!app.hollywood.editing);
    assert_eq!(app.hollywood.input.as_str(), "hi");
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("acme-shop".into()))
    );

    // `Shift+Tab` (`BackTab`) reverses back to `Waiting` (`c3`), then `Compose` (`Input`) without
    // losing the draft.
    key_mods(&mut app, hc_vt::Key::BackTab, hc_vt::Mods::SHIFT);
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
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
    // ↓ walks past Talk(8) -> HollywoodLive -> Input and clamps at Input.
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
        ]
    );
    // ↑ walks smoothly to the top of `Lower` (`Headline`) without jumping to `Upper`.
    for _ in 0..8 {
        key(&mut app, hc_vt::Key::ArrowUp);
    }
    assert_eq!(app.hollywood.selected, Some(Target::Headline));
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.hollywood.selected, Some(Target::Hollywood));
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

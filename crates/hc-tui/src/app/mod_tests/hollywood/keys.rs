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
    assert_eq!(app.hollywood.mode, LayerMode::Expanded, "the entry state");
    // The strip is the state this test toggles from.
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
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
    alt_y(&mut app);
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
    // `alt+up` is `previous_studio` — refused, never echoed.
    app.set_hollywood(
        Ok(Some(cfg(Some("alt+up")))),
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
    // Opening selects the first frame.
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("acme-shop".into()))
    );
    // → / l walk the frames, the new-studio frame included.
    key(&mut app, hc_vt::Key::Char('l'));
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("beta-tools".into()))
    );
    key(&mut app, hc_vt::Key::Char('l'));
    assert_eq!(app.hollywood.selected, Some(Target::NewStudio));
    // The last frame is the end of the line.
    key(&mut app, hc_vt::Key::Char('l'));
    assert_eq!(app.hollywood.selected, Some(Target::NewStudio));
    // ← / h come back.
    key(&mut app, hc_vt::Key::Char('h'));
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("beta-tools".into()))
    );
    // ↓ leaves the frames for the band's first row; ↑ comes back.
    key(&mut app, hc_vt::Key::Char('j'));
    assert_eq!(app.hollywood.selected, Some(Target::Headline));
    key(&mut app, hc_vt::Key::Char('j'));
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
    key(&mut app, hc_vt::Key::Char('k'));
    assert_eq!(app.hollywood.selected, Some(Target::Headline));
    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(
        app.hollywood.selected,
        Some(Target::NewStudio),
        "up from the band returns to the frames"
    );
    // Nothing reached the focused program.
    assert_eq!(app.pane.input_backlog().0, 0, "expanded keys leaked");
}

#[tokio::test]
async fn x_hides_and_esc_collapses() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    alt_y(&mut app);
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    alt_y(&mut app);
    key(&mut app, hc_vt::Key::Char('x'));
    assert_eq!(app.hollywood.mode, LayerMode::Hidden);
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
    key(&mut app, hc_vt::Key::Char('i'));
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
    key(&mut app, hc_vt::Key::Char('n'));
    assert_eq!(
        app.hollywood.mode,
        LayerMode::Collapsed,
        "the popup is never under the bar"
    );
    assert!(app.popup_pending.is_some(), "the wizard is armed");
}

#[tokio::test]
async fn enter_on_the_new_studio_frame_opens_the_wizard() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    alt_y(&mut app);
    app.hollywood.selected = Some(Target::NewStudio);
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    assert!(app.popup_pending.is_some(), "the wizard is armed");
}

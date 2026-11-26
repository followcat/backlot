//! Float sizing from the current `[[keys.command]]` binding across
//! adoption, show, hide, config reload, removed bindings, and host resize.
use super::floats::{base_terms, command_labeled, float_term, publish, two_spaces};
use super::*;
use crate::app::float::FloatKey;
use crate::popup_size::{PopupResolvedGeometry, PopupSize, resolve_popup_geometry};
use crate::test_support::TestDir;
use hc_vt::{Action as KeyAction, Key, Mods};
use ratatui::layout::Rect;

pub(super) fn live_with_popup(
    width: Option<&str>,
    height: Option<&str>,
) -> crate::config::LiveKeybindConfig {
    let mut text = String::from(
        "[[keys.command]]\n\
         key = \"alt+f\"\n\
         type = \"popup\"\n\
         command = 'exec \"${SHELL:-sh}\"'\n",
    );
    if let Some(w) = width {
        text.push_str(&format!("width = \"{w}\"\n"));
    }
    if let Some(h) = height {
        text.push_str(&format!("height = \"{h}\"\n"));
    }
    let config: crate::config::Config = toml::from_str(&text).unwrap();
    config.live_keybinds()
}

pub(super) fn press_alt_f(app: &mut App) {
    let width = app.area.0;
    app.on_input(
        UiEvent::Key(term_key(Key::Char('f'), Mods::ALT, KeyAction::Press)),
        width,
    );
}

fn find_cell(backend: &ratatui::backend::TestBackend, symbol: &str) -> Option<(u16, u16)> {
    let size = backend.size().unwrap();
    let buf = backend.buffer();
    (0..size.height)
        .flat_map(|y| (0..size.width).map(move |x| (x, y)))
        .find(|&(x, y)| buf[(x, y)].symbol() == symbol)
}

fn drawn_frame_rect(app: &mut App) -> Rect {
    let (w, h) = app.area;
    app.notice = None;
    let backend = draw_to(app, w, h);
    let (x0, y0) = find_cell(&backend, "╭").expect("float top-left corner");
    let (x1, _) = find_cell(&backend, "╮").expect("float top-right corner");
    let (_, y1) = find_cell(&backend, "╰").expect("float bottom-left corner");
    Rect::new(x0, y0, x1 - x0 + 1, y1 - y0 + 1)
}

pub(super) fn expected_geometry(
    app: &App,
    width: Option<PopupSize>,
    height: Option<PopupSize>,
) -> PopupResolvedGeometry {
    resolve_popup_geometry(width, height, app.frame_area()).expect("popup fits frame")
}

#[tokio::test]
async fn adopted_float_with_80_percent_binding_resolves_frame_and_terminal_size() {
    let mut app = two_spaces();
    app.area = (120, 40);
    app.live = live_with_popup(Some("80%"), Some("80%"));
    let owner = app.focused_owner_key().unwrap();
    let key = FloatKey {
        owner: owner.clone(),
        command: "alt+f".into(),
    };
    let expected = expected_geometry(
        &app,
        Some(PopupSize::Percent(80)),
        Some(PopupSize::Percent(80)),
    );
    let half = expected_geometry(&app, None, None);
    assert!(expected.frame.width > half.frame.width);

    // Core reports a smaller stale terminal size from before the restart.
    let mut t9 = float_term("t9", &owner, 9);
    t9.cols = 40;
    t9.rows = 12;
    let mut terms = base_terms();
    terms.push(t9);
    publish(&mut app, terms);

    let entry = app.floats.get(&key).expect("float adopted");
    assert_eq!(entry.width, Some(PopupSize::Percent(80)));
    assert_eq!(entry.height, Some(PopupSize::Percent(80)));
    assert_eq!(
        app.size_leases["t9"].allocated_size(),
        (expected.terminal_cols, expected.terminal_rows),
        "adopted hidden lease must take the 80% terminal size"
    );

    // Next show draws at 80% and allocates the popup pane at 80% terminal size.
    press_alt_f(&mut app);
    let popup = app.popup.as_ref().expect("float shown");
    assert_eq!(popup.width, Some(PopupSize::Percent(80)));
    assert_eq!(popup.height, Some(PopupSize::Percent(80)));
    assert_eq!(
        popup.pane.allocated_size(),
        (expected.terminal_cols, expected.terminal_rows)
    );
    assert_eq!(drawn_frame_rect(&mut app), expected.frame);
    let hit = app.hit_map.popup.as_ref().expect("popup hit rect");
    assert!(hit.inner_rect.width >= expected.terminal_cols);
    assert_eq!(hit.inner_rect.height, expected.terminal_rows);

    // Hiding keeps the SizeLease at the same 80% terminal size.
    press_alt_f(&mut app);
    assert!(app.popup.is_none());
    assert_eq!(
        app.size_leases["t9"].allocated_size(),
        (expected.terminal_cols, expected.terminal_rows)
    );
}

#[tokio::test]
async fn binding_size_change_applies_on_next_show() {
    let mut app = two_spaces();
    app.area = (120, 40);
    app.live = live_with_popup(Some("40%"), Some("40%"));
    let owner = app.focused_owner_key().unwrap();
    let key = FloatKey {
        owner: owner.clone(),
        command: "alt+f".into(),
    };
    let geom_40 = expected_geometry(
        &app,
        Some(PopupSize::Percent(40)),
        Some(PopupSize::Percent(40)),
    );
    let geom_80 = expected_geometry(
        &app,
        Some(PopupSize::Percent(80)),
        Some(PopupSize::Percent(80)),
    );

    press_alt_f(&mut app);
    assert!(app.popup_pending.is_some());
    app.on_popup_spawned("t9".into(), 9);
    let mut terms = base_terms();
    terms.push(float_term("t9", &owner, 9));
    publish(&mut app, terms);

    assert_eq!(
        app.popup.as_ref().unwrap().pane.allocated_size(),
        (geom_40.terminal_cols, geom_40.terminal_rows)
    );
    assert_eq!(drawn_frame_rect(&mut app), geom_40.frame);

    // Hide, update the binding size, then show again.
    press_alt_f(&mut app);
    assert!(app.floats.contains_key(&key));
    app.live = live_with_popup(Some("80%"), Some("80%"));
    press_alt_f(&mut app);

    let popup = app.popup.as_ref().expect("float re-shown");
    assert_eq!(popup.width, Some(PopupSize::Percent(80)));
    assert_eq!(popup.height, Some(PopupSize::Percent(80)));
    assert_eq!(
        popup.pane.allocated_size(),
        (geom_80.terminal_cols, geom_80.terminal_rows)
    );
    assert_eq!(drawn_frame_rect(&mut app), geom_80.frame);

    // A live config reload also updates an already-shown float immediately.
    let dir = TestDir::new("hctui-float-size-reload");
    app.shell.local_config_path = dir.join("config.toml");
    std::fs::write(
        &app.shell.local_config_path,
        "[[keys.command]]\n\
         key = \"alt+f\"\n\
         type = \"popup\"\n\
         command = 'exec \"${SHELL:-sh}\"'\n\
         width = \"60%\"\n\
         height = \"70%\"\n",
    )
    .unwrap();
    let geom_reloaded = expected_geometry(
        &app,
        Some(PopupSize::Percent(60)),
        Some(PopupSize::Percent(70)),
    );
    app.on_action(crate::input::KeybindAction::ReloadConfig, 120);
    let popup = app.popup.as_ref().expect("float remains shown");
    assert_eq!(
        popup.pane.allocated_size(),
        (geom_reloaded.terminal_cols, geom_reloaded.terminal_rows)
    );
    assert_eq!(drawn_frame_rect(&mut app), geom_reloaded.frame);
}

#[tokio::test]
async fn removed_binding_falls_back_to_default() {
    let mut app = two_spaces();
    app.area = (120, 40);
    app.live = crate::config::Config::default().live_keybinds();
    let owner = app.focused_owner_key().unwrap();
    let key = FloatKey {
        owner: owner.clone(),
        command: "alt+f".into(),
    };
    let default_geom = expected_geometry(&app, None, None);

    // Adopt a float whose binding is no longer in config.
    let mut t9 = float_term("t9", &owner, 9);
    t9.cols = 132;
    t9.rows = 43;
    let mut terms = base_terms();
    terms.push(t9);
    publish(&mut app, terms);

    let entry = app.floats.get(&key).expect("float adopted");
    assert_eq!(entry.width, None);
    assert_eq!(entry.height, None);
    assert_eq!(
        app.size_leases["t9"].allocated_size(),
        (default_geom.terminal_cols, default_geom.terminal_rows),
        "adopted float with removed binding resolves to default size"
    );

    // Showing it uses the default half-surface geometry.
    app.floats.get_mut(&key).unwrap().restore = true;
    app.focused = Some("t2".into());
    app.reconcile_published_view();
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    let popup = app.popup.as_ref().expect("float promoted");
    assert_eq!(
        popup.pane.allocated_size(),
        (default_geom.terminal_cols, default_geom.terminal_rows)
    );
    assert_eq!(drawn_frame_rect(&mut app), default_geom.frame);

    // Removing width/height from an existing binding also falls back to default,
    // whereas removing the binding entirely keeps a previously stored explicit size.
    let mut stored_app = two_spaces();
    stored_app.area = (120, 40);
    let stored_owner = stored_app.focused_owner_key().unwrap();
    let mut custom = command_labeled("alt+f", "exec \"${SHELL:-sh}\"");
    custom.width = Some(PopupSize::Percent(80));
    custom.height = Some(PopupSize::Percent(80));
    stored_app.run_custom_command(&custom);
    stored_app.on_popup_spawned("t8".into(), 8);
    let mut stored_terms = base_terms();
    stored_terms.push(float_term("t8", &stored_owner, 8));
    publish(&mut stored_app, stored_terms);
    let geom_80 = expected_geometry(
        &stored_app,
        Some(PopupSize::Percent(80)),
        Some(PopupSize::Percent(80)),
    );
    // Binding removed completely -> stored 80% still applies.
    stored_app.live = crate::config::Config::default().live_keybinds();
    stored_app.popup_resize();
    assert_eq!(
        stored_app.popup.as_ref().unwrap().pane.allocated_size(),
        (geom_80.terminal_cols, geom_80.terminal_rows)
    );
    // Binding still present with default width/height -> default size applies.
    stored_app.live = live_with_popup(None, None);
    stored_app.popup_resize();
    assert_eq!(
        stored_app.popup.as_ref().unwrap().pane.allocated_size(),
        (default_geom.terminal_cols, default_geom.terminal_rows)
    );
}

#[tokio::test]
async fn hidden_float_lease_follows_host_resize_at_configured_size() {
    let mut app = two_spaces();
    app.area = (100, 30);
    app.live = live_with_popup(Some("80%"), Some("80%"));
    let owner = app.focused_owner_key().unwrap();
    let geom_small = expected_geometry(
        &app,
        Some(PopupSize::Percent(80)),
        Some(PopupSize::Percent(80)),
    );

    let mut t9 = float_term("t9", &owner, 9);
    t9.cols = 40;
    t9.rows = 12;
    let mut terms = base_terms();
    terms.push(t9);
    publish(&mut app, terms);

    assert_eq!(
        app.size_leases["t9"].allocated_size(),
        (geom_small.terminal_cols, geom_small.terminal_rows)
    );

    app.on_input(
        UiEvent::Resize {
            cols: 160,
            rows: 48,
        },
        160,
    );
    let geom_large = expected_geometry(
        &app,
        Some(PopupSize::Percent(80)),
        Some(PopupSize::Percent(80)),
    );
    let half_large = expected_geometry(&app, None, None);
    assert!(geom_large.terminal_cols > half_large.terminal_cols);
    assert_eq!(
        app.size_leases["t9"].allocated_size(),
        (geom_large.terminal_cols, geom_large.terminal_rows),
        "hidden lease must resize to 80% of the new frame, not default half"
    );
}

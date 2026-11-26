//! Discoverability tests for `[keys] focus_lot`: lot header hint
//! (default, rebound, and unbound), `Ctrl+B ?` help launcher row and
//! activation, and classic `?` backlot help group.

use super::focus_lot::app_with_studio_and_talk;
use super::*;
use crate::panel::{PanelMode, Target};
use crate::test_support::TestDir;

#[tokio::test]
async fn alt_l_hint_shows_in_lot_header_help_and_launcher() {
    let mut app = app_with_studio_and_talk();
    app.area = (140, 40);
    app.panel.mode = PanelMode::Expanded;

    // 1. Lot header at default dock widths (140 -> 38 cols, 120 -> 36 cols)
    // shows `alt+l focus` (no `alt+y / ctrl+b i` sheet hint in Lot header).
    for (cols, rows) in [(140u16, 40u16), (120, 24)] {
        app.area = (cols, rows);
        let screen = screen_lines(&draw_to(&mut app, cols, rows)).join("\n");
        let header = screen.lines().next().unwrap_or_default();
        let lot_header = header.rsplit('│').next().unwrap_or_default();
        assert!(
            lot_header.contains("lot · 1"),
            "{cols}x{rows} header: {header}"
        );
        assert!(
            lot_header.contains("alt+l focus"),
            "{cols}x{rows} header: {header}"
        );
        assert!(
            !lot_header.contains("alt+y"),
            "{cols}x{rows} lot header should not repeat alt+y: {header}"
        );
    }

    // 2. Help launcher (`Ctrl+B ?` with studios) lists alt+l and focuses the lot on Enter.
    let groups = app.help_groups();
    let has_launcher_hint = groups
        .iter()
        .flat_map(|g| g.rows.iter())
        .any(|r| r.hint == "alt+l");
    assert!(has_launcher_hint, "launcher must advertise alt+l");

    app.help = true;
    let rows = app.help_rows();
    app.help_state.selected = rows
        .iter()
        .position(|r| r.hint == "alt+l")
        .expect("alt+l row in help launcher");
    app.panel.mode = PanelMode::Closed;
    app.panel.focused = false;
    app.help_launcher_activate();
    assert!(!app.help);
    assert_eq!(app.panel.mode, PanelMode::Expanded);
    assert!(app.panel.focused);
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));

    // 3. Classic help (`backlot_help_groups`) lists alt+l beside alt+y / ctrl+b i.
    let classic = crate::keybind_help::backlot_help_groups(
        &app.live.keybinds,
        &app.live.prefix,
        &app.panel_config,
        app.hollywood_key.unwrap_or_default(),
        app.panel_key_combo,
    );
    let backlot_group = classic
        .iter()
        .find(|(title, _)| *title == "backlot")
        .expect("backlot help group");
    assert!(
        backlot_group.1.iter().any(|(k, _)| k == "alt+l"),
        "classic help needs alt+l: {:?}",
        backlot_group.1
    );

    // 4. Rebound/unbound focus_lot updates lot header and launcher hints.
    let dir = TestDir::new("focus-lot-hints");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "[keys]\nfocus_lot = \"alt+o\"\n").unwrap();
    let (config, notice) = crate::panel::load(&path);
    assert!(notice.is_none());
    app.set_panel_config(config, notice, &path);
    app.panel.focused = false;
    app.area = (140, 40);
    let screen = screen_lines(&draw_to(&mut app, 140, 40)).join("\n");
    let header = screen.lines().next().unwrap_or_default();
    let lot_header = header.rsplit('│').next().unwrap_or_default();
    assert!(lot_header.contains("alt+o"), "rebound lot header: {header}");

    std::fs::write(&path, "[keys]\nfocus_lot = \"\"\n").unwrap();
    let (config, notice) = crate::panel::load(&path);
    assert!(notice.is_none());
    app.set_panel_config(config, notice, &path);
    let screen = screen_lines(&draw_to(&mut app, 140, 40)).join("\n");
    let header = screen.lines().next().unwrap_or_default();
    let lot_header = header.rsplit('│').next().unwrap_or_default();
    assert!(!lot_header.contains("alt+l"), "unbound header: {header}");
    assert!(
        !lot_header.contains("alt+y"),
        "unbound header omits removed alt+y hint: {header}"
    );
}

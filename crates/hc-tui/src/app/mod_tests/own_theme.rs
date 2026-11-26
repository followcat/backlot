//! backlot's own `[theme]` through the app: the Settings list, where a
//! picked theme is saved, and what a reload reads.
use super::*;
use crate::input::KeybindAction as A;
use crate::test_support::TestDir;
use hc_vt::{Action, Key, Mods};

fn key(app: &mut App, key: Key) {
    app.on_input(
        input::UiEvent::Key(term_key(key, Mods::NONE, Action::Press)),
        120,
    );
}

/// Scratch homes for both files: herdr's `config.toml` with `herdr`,
/// backlot's with `own` (`None`: no file). Returns backlot's path.
fn files(app: &mut App, herdr: &str, own: Option<&str>) -> (TestDir, std::path::PathBuf) {
    let dir = TestDir::new("backlot-own-theme-app");
    std::fs::create_dir_all(dir.join("herdr")).unwrap();
    app.shell.local_config_path = dir.join("herdr/config.toml");
    std::fs::write(&app.shell.local_config_path, herdr).unwrap();
    let own_path = dir.join("backlot/config.toml");
    if let Some(own) = own {
        std::fs::create_dir_all(own_path.parent().unwrap()).unwrap();
        std::fs::write(&own_path, own).unwrap();
    }
    app.shell.own_theme = crate::own_theme::Shell::at(own_path.clone());
    app.reload_config();
    (dir, own_path)
}

#[tokio::test]
async fn a_reload_reads_backlots_theme_and_not_herdrs() {
    let mut app = app();
    let (_root, own) = files(&mut app, "[theme]\nname = \"nord\"\n", None);
    assert_eq!(app.shell.theme_name, "backlot");
    assert_eq!(
        app.shell.palette,
        crate::chrome::palette::Palette::backlot()
    );

    std::fs::create_dir_all(own.parent().unwrap()).unwrap();
    std::fs::write(&own, "[theme]\nname = \"dracula\"\n").unwrap();
    app.reload_config();
    assert_eq!(app.shell.theme_name, "dracula");

    std::fs::write(&own, "[theme]\nsource = \"herdr\"\n").unwrap();
    app.reload_config();
    assert_eq!(app.shell.theme_name, "nord", "herdr's theme, as hc-tui");

    std::fs::remove_file(&own).unwrap();
    app.reload_config();
    assert_eq!(app.shell.theme_name, "backlot");
}

#[tokio::test]
async fn settings_lists_backlot_first_and_saves_the_pick_in_backlots_file() {
    let mut app = app();
    let herdr = "[theme]\nname = \"nord\"\n";
    let (_root, own) = files(&mut app, herdr, None);
    app.on_action(A::Settings, 120);
    let settings = app.settings.as_ref().expect("the overlay");
    assert_eq!(settings.theme_names[0], "backlot");
    assert_eq!(settings.selected, 0, "the theme in force is selected");
    let screen = screen_lines(&draw_to(&mut app, 120, 40)).join("\n");
    assert!(screen.contains("backlot ✓"), "{screen}");

    // One row down is the first reference theme; Enter saves it.
    key(&mut app, Key::Char('j'));
    assert_eq!(app.shell.theme_name, crate::config::THEME_NAMES[0]);
    key(&mut app, Key::Enter);
    assert!(app.settings.is_none(), "a theme apply closes the overlay");
    let written = std::fs::read_to_string(&own).unwrap();
    assert!(
        written.contains("[theme]") && written.contains("name = \"catppuccin\""),
        "{written}"
    );
    assert_eq!(
        std::fs::read_to_string(&app.shell.local_config_path).unwrap(),
        herdr,
        "herdr's file is not written"
    );
    assert_eq!(app.shell.theme_name, "catppuccin");
}

#[tokio::test]
async fn with_source_herdr_the_list_and_the_save_are_the_references() {
    let mut app = app();
    let own_text = "[theme]\nsource = \"herdr\"\n";
    let (_root, own) = files(&mut app, "[theme]\nname = \"nord\"\n", Some(own_text));
    app.on_action(A::Settings, 120);
    let settings = app.settings.as_ref().expect("the overlay");
    assert_eq!(settings.theme_names, crate::config::THEME_NAMES);
    let nord = crate::config::THEME_NAMES
        .iter()
        .position(|name| *name == "nord")
        .unwrap();
    assert_eq!(settings.selected, nord);
    let screen = screen_lines(&draw_to(&mut app, 120, 40)).join("\n");
    assert!(!screen.contains("backlot"), "{screen}");

    key(&mut app, Key::Char('j'));
    key(&mut app, Key::Enter);
    let next = crate::config::THEME_NAMES[nord + 1];
    let written = std::fs::read_to_string(&app.shell.local_config_path).unwrap();
    assert!(written.contains(&format!("name = \"{next}\"")), "{written}");
    assert_eq!(std::fs::read_to_string(&own).unwrap(), own_text);
    assert_eq!(app.shell.theme_name, next);
}

/// A reload that swaps the theme repaints every palette cell: the
/// panel's background and the frame's mode bar take the new palette
/// while a warm pane-surface cache keeps its own VT colours — neither
/// cache may hold the old theme's.
#[tokio::test]
async fn a_theme_reload_repaints_the_drawn_frame() {
    let mut app = app_with_terms(vec![{
        let mut t = crate::sidebar::fixture::term("t1");
        t.labels.insert("herdr.space".into(), "space-a".into());
        t.labels.insert("herdr.tab".into(), "tab-a".into());
        t
    }]);
    let (_root, own) = files(&mut app, "", None);
    // A parked attach keeps the pane's VT as a retained screen — the
    // render cache then has real content to hold across the reload.
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    app.focused = Some("t1".into());
    app.sync_target();
    let mut vt = hc_vt::Vt::new(90, 22, 0);
    vt.feed(b"\x1b[31mPANE-RED\x1b[0m");
    app.pane.install_vt(vt);
    app.area = (120, 24);
    app.panel.mode = crate::panel::PanelMode::Expanded;
    let backlot_bg = crate::chrome::palette::Palette::backlot()
        .elevation()
        .surface;
    let dracula_bg = crate::chrome::palette::Palette::dracula()
        .elevation()
        .surface;
    assert_ne!(backlot_bg, dracula_bg, "the probe must differ");

    let before = draw_to(&mut app, 120, 24);
    let inner = app.hit_map.panes[0].inner_rect;
    let pane_cell = |b: &TestBackend| b.buffer()[(inner.x, inner.y)].fg;
    let panel_cell = |b: &TestBackend| b.buffer()[(110, 10)].bg;
    assert_eq!(panel_cell(&before), backlot_bg);
    let vt_red = pane_cell(&before);

    std::fs::create_dir_all(own.parent().unwrap()).unwrap();
    std::fs::write(&own, "[theme]\nname = \"dracula\"\n").unwrap();
    app.reload_config();
    let after = draw_to(&mut app, 120, 24);
    assert_eq!(
        panel_cell(&after),
        dracula_bg,
        "the redrawn frame kept the old palette"
    );
    assert_eq!(
        pane_cell(&after),
        vt_red,
        "the cached pane surface lost its own colours"
    );
}

#[tokio::test]
async fn the_other_settings_still_go_to_herdrs_file() {
    let mut app = app();
    let (_root, own) = files(&mut app, "", None);
    app.on_action(A::Settings, 120);
    key(&mut app, Key::Tab); // Indicators
    key(&mut app, Key::Char('j'));
    key(&mut app, Key::Enter);
    let written = std::fs::read_to_string(&app.shell.local_config_path).unwrap();
    assert!(written.contains("status_indicators"), "{written}");
    assert!(!own.exists(), "backlot's file is written for a theme only");
    assert_eq!(app.shell.theme_name, "backlot", "a reload keeps the theme");
}

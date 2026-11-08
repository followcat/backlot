use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

struct Scratch(std::path::PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn config(app: &mut App, text: &str) -> Scratch {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "hctui-reload-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&path).unwrap();
    app.shell.local_config_path = path.join("config.toml");
    std::fs::write(&app.shell.local_config_path, text).unwrap();
    Scratch(path)
}
fn key(app: &mut App, ch: char, mods: hc_vt::Mods) {
    app.on_input(
        UiEvent::Key(input::TerminalKey::new(hc_vt::KeyInput {
            key: hc_vt::Key::Char(ch),
            mods,
            action: hc_vt::Action::Press,
        })),
        120,
    );
}
fn rendered(app: &mut App, width: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
    terminal
        .draw(|frame| draw::draw(app, frame, Instant::now()))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[tokio::test]
async fn reload_applies_binding_theme_geometry_without_writing_the_file() {
    let mut app = app();
    let text = "[keys]\nprefix='ctrl+a'\nhelp='prefix+shift+k'\n[theme]\nname='nord'\n[ui]\nsidebar_width=32\n";
    let _root = config(&mut app, text);
    app.on_action(crate::input::KeybindAction::ReloadConfig, 120);
    assert_eq!(app.shell.theme_name, "nord");
    assert_eq!(app.sidebar_width, 32);
    assert!(app.shell.startup_config_diagnostic.is_none());
    key(&mut app, 'a', hc_vt::Mods::CTRL);
    key(&mut app, 'K', hc_vt::Mods::SHIFT);
    assert!(app.help, "new prefix/binding must route immediately");
    assert_eq!(
        std::fs::read_to_string(&app.shell.local_config_path).unwrap(),
        text
    );
}

#[tokio::test]
async fn invalid_sections_keep_current_values_while_valid_siblings_reload() {
    let mut app = app();
    let _root = config(
        &mut app,
        "[keys]\nprefix='ctrl+a'\n[ui]\nsidebar_width='invalid'\n[theme]\nname='nord'\n",
    );
    let old_width = app.sidebar_width;
    app.on_action(crate::input::KeybindAction::ReloadConfig, 120);
    assert_eq!(app.sidebar_width, old_width);
    assert_eq!(app.shell.theme_name, "nord");
    key(&mut app, 'a', hc_vt::Mods::CTRL);
    key(&mut app, '?', hc_vt::Mods::NONE);
    assert!(app.help);
    assert!(app.shell.startup_config_diagnostic.is_some());
}

#[tokio::test]
async fn invalid_prefix_keeps_current_bindings_and_parse_failure_keeps_everything() {
    let mut app = app();
    let _root = config(
        &mut app,
        "[keys]\nprefix='invalid-key'\n[theme]\nname='nord'\n",
    );
    app.on_action(crate::input::KeybindAction::ReloadConfig, 120);
    assert_eq!(app.shell.theme_name, "nord");
    key(&mut app, 'b', hc_vt::Mods::CTRL);
    key(&mut app, '?', hc_vt::Mods::NONE);
    assert!(app.help);
    std::fs::write(&app.shell.local_config_path, "[invalid").unwrap();
    app.on_action(crate::input::KeybindAction::ReloadConfig, 120);
    assert_eq!(app.shell.theme_name, "nord");
    assert!(
        app.shell
            .startup_config_diagnostic
            .as_deref()
            .unwrap()
            .contains("keeping current config")
    );
}

#[tokio::test]
async fn reload_preserves_manual_sidebar_width_and_missing_file_restores_defaults() {
    let mut app = app();
    let _root = config(&mut app, "[theme]\nname='nord'\n[ui]\nsidebar_width=32\n");
    app.set_sidebar_width_from_column(29);
    app.on_action(crate::input::KeybindAction::ReloadConfig, 120);
    assert_eq!(app.sidebar_width, 30);
    assert_eq!(app.shell.sidebar_width, 32);
    std::fs::remove_file(&app.shell.local_config_path).unwrap();
    app.on_action(crate::input::KeybindAction::ReloadConfig, 120);
    assert_eq!(app.shell.theme_name, "catppuccin");
    assert_eq!(app.sidebar_width, 30);
}

#[tokio::test]
async fn settings_binding_and_mobile_menu_open_the_settings_overlay() {
    let mut app = app();
    key(&mut app, 'b', hc_vt::Mods::CTRL);
    key(&mut app, 's', hc_vt::Mods::NONE);
    assert!(app.settings.is_some());
    assert!(rendered(&mut app, 120).contains("settings"));
    app.cancel_settings_overlay();
    assert!(app.settings.is_none());
    app.on_mobile_target(&crate::chrome::render::ClientMobileTarget::Menu(0));
    assert!(app.settings.is_some());
    assert!(rendered(&mut app, 40).contains("settings"));
}

#[tokio::test]
async fn reload_preserves_manually_selected_agent_order() {
    let mut app = app();
    let _root = config(
        &mut app,
        "[ui]\nagent_panel_sort='spaces'\n[theme]\nname='nord'\n",
    );
    app.flip_agent_sort();
    assert_eq!(
        app.shell.agent_panel_sort,
        crate::config::AgentPanelSortConfig::Priority
    );
    app.on_action(crate::input::KeybindAction::ReloadConfig, 120);
    assert_eq!(
        app.shell.agent_panel_sort,
        crate::config::AgentPanelSortConfig::Priority
    );
    assert_eq!(app.shell.theme_name, "nord");
}

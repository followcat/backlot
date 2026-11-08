//! M4 slice 4: the Settings overlay and the agent-menu operations.
use super::*;
use crate::app::input::{MouseButton, MouseEventKind};
use crate::chrome::settings::SettingsSection;
use crate::input::KeybindAction as A;
use hc_vt::{Action, Key, Mods};

fn press(key: Key) -> input::TerminalKey {
    term_key(key, Mods::NONE, Action::Press)
}

fn key(app: &mut App, key: Key) {
    app.on_input(input::UiEvent::Key(press(key)), 120);
}

fn screen(app: &mut App) -> Vec<String> {
    screen_lines(&draw_to(app, 120, 40))
}

struct Scratch(std::path::PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A temp config file bound as the app's `local_config_path` — the only
/// file the Settings save path may ever write.
fn settings_file(app: &mut App, text: &str) -> Scratch {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "hctui-settings-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    app.shell.local_config_path = dir.join("config.toml");
    std::fs::write(&app.shell.local_config_path, text).unwrap();
    Scratch(dir)
}

fn open_settings(app: &mut App) {
    app.on_action(A::Settings, 120);
    assert!(app.settings.is_some(), "A::Settings opens the overlay");
}

#[tokio::test]
async fn settings_opens_on_the_binding_and_moves_through_every_section() {
    let mut app = app();
    let _root = settings_file(&mut app, "");
    // The real route: prefix arms, `s` resolves the Settings binding.
    app.on_input(
        input::UiEvent::Key(term_key(Key::Char('b'), Mods::CTRL, Action::Press)),
        120,
    );
    key(&mut app, Key::Char('s'));
    assert_eq!(
        app.settings.as_ref().map(|s| s.section),
        Some(SettingsSection::Theme)
    );
    let rendered = screen(&mut app).join("\n");
    assert!(rendered.contains("settings"));
    assert!(rendered.contains("catppuccin"));
    for (press_key, section) in [
        (Key::Tab, SettingsSection::Indicators),
        (Key::Tab, SettingsSection::Sound),
        (Key::Tab, SettingsSection::Toast),
        (Key::Tab, SettingsSection::Integrations),
        (Key::Tab, SettingsSection::Theme),
        (Key::BackTab, SettingsSection::Integrations),
        (Key::Char('h'), SettingsSection::Toast),
        (Key::Char('l'), SettingsSection::Integrations),
        (Key::ArrowLeft, SettingsSection::Toast),
        (Key::ArrowRight, SettingsSection::Integrations),
    ] {
        key(&mut app, press_key);
        assert_eq!(
            app.settings.as_ref().map(|s| s.section),
            Some(section),
            "{press_key:?}"
        );
    }
    key(&mut app, Key::Escape);
    assert!(app.settings.is_none(), "Esc closes the overlay");
}

#[tokio::test]
async fn settings_indicator_edit_writes_toml_and_the_reload_applies_it() {
    let mut app = app();
    let _root = settings_file(&mut app, "# a user comment\n[ui]\nsidebar_width = 30\n");
    open_settings(&mut app);
    key(&mut app, Key::Tab);
    assert_eq!(
        app.settings.as_ref().map(|s| (s.section, s.selected)),
        Some((SettingsSection::Indicators, 0))
    );
    key(&mut app, Key::Char('j'));
    key(&mut app, Key::Enter);
    let written = std::fs::read_to_string(&app.shell.local_config_path).unwrap();
    assert!(
        written.contains("status_indicators = \"symbols\""),
        "{written}"
    );
    assert!(written.contains("# a user comment"), "{written}");
    assert!(written.contains("sidebar_width = 30"), "{written}");
    assert_eq!(
        app.shell.status_indicators,
        crate::config::StatusIndicatorStyle::Symbols,
        "the save reloads the live config"
    );
    assert!(
        app.settings.is_some(),
        "a radio-section apply keeps the overlay open"
    );
}

#[tokio::test]
async fn settings_theme_apply_writes_name_and_auto_switch_then_closes() {
    let mut app = app();
    let _root = settings_file(&mut app, "");
    open_settings(&mut app);
    key(&mut app, Key::Char('j'));
    assert_eq!(app.shell.theme_name, crate::config::THEME_NAMES[1]);
    key(&mut app, Key::Enter);
    let written = std::fs::read_to_string(&app.shell.local_config_path).unwrap();
    assert!(
        written.contains("[theme]") && written.contains("name = \"catppuccin-latte\""),
        "{written}"
    );
    assert!(written.contains("auto_switch = false"), "{written}");
    assert!(app.settings.is_none(), "a theme apply closes the overlay");
    assert_eq!(app.shell.theme_name, "catppuccin-latte");
}

#[tokio::test]
async fn settings_escape_restores_the_previewed_theme_without_writing() {
    let mut app = app();
    let _root = settings_file(&mut app, "");
    let original = app.shell.palette.clone();
    open_settings(&mut app);
    key(&mut app, Key::Char('j'));
    assert_ne!(app.shell.theme_name, "catppuccin");
    key(&mut app, Key::Escape);
    assert!(app.settings.is_none());
    assert_eq!(app.shell.theme_name, "catppuccin");
    assert_eq!(
        app.shell.palette, original,
        "cancel restores the palette snapshot"
    );
    assert_eq!(
        std::fs::read_to_string(&app.shell.local_config_path).unwrap(),
        ""
    );
}

#[tokio::test]
async fn settings_integrations_renders_unavailable_and_apply_is_inert() {
    let mut app = app();
    let _root = settings_file(&mut app, "");
    open_settings(&mut app);
    for _ in 0..4 {
        key(&mut app, Key::Tab);
    }
    assert_eq!(
        app.settings.as_ref().map(|s| s.section),
        Some(SettingsSection::Integrations)
    );
    let rendered = screen(&mut app).join("\n");
    assert!(
        rendered.contains("no integration targets available"),
        "{rendered}"
    );
    key(&mut app, Key::Enter);
    assert!(app.notice.is_none(), "an inert apply raises no notice");
    assert_eq!(
        std::fs::read_to_string(&app.shell.local_config_path).unwrap(),
        ""
    );
}

#[tokio::test]
async fn settings_sound_and_toast_edits_write_their_sections() {
    let mut app = app();
    let _root = settings_file(&mut app, "[ui.toast]\nenabled = true\n");
    open_settings(&mut app);
    app.select_settings_section(SettingsSection::Sound);
    key(&mut app, Key::Char('j'));
    key(&mut app, Key::Enter);
    let written = std::fs::read_to_string(&app.shell.local_config_path).unwrap();
    assert!(
        written.contains("[ui.sound]") && written.contains("enabled = false"),
        "{written}"
    );
    assert!(!app.shell.sound.enabled);
    app.select_settings_section(SettingsSection::Toast);
    app.select_settings_choice(2);
    key(&mut app, Key::Enter);
    let written = std::fs::read_to_string(&app.shell.local_config_path).unwrap();
    assert!(written.contains("delivery = \"terminal\""), "{written}");
    // `write_edit` drops the legacy `ui.toast.enabled` flag.
    assert!(!written.contains("enabled = true"), "{written}");
    assert_eq!(
        app.shell.toast_delivery,
        crate::config::ToastDelivery::Terminal
    );
}

#[tokio::test]
async fn settings_mouse_clicks_sections_choices_and_cancel() {
    let mut app = app();
    let _root = settings_file(&mut app, "");
    open_settings(&mut app);
    draw_to(&mut app, 120, 40);
    let toast = app
        .settings_hits
        .tabs
        .iter()
        .find(|(_, s)| *s == SettingsSection::Toast)
        .map(|(r, _)| (r.x, r.y))
        .expect("toast tab hit");
    app.on_input(
        input::UiEvent::Mouse(input::MouseInput {
            kind: MouseEventKind::Down(MouseButton::Left),
            col: toast.0,
            row: toast.1,
            mods: Mods::NONE,
            sgr_position: None,
        }),
        120,
    );
    assert_eq!(
        app.settings.as_ref().map(|s| s.section),
        Some(SettingsSection::Toast)
    );
    let choice = app
        .settings_hits
        .choices
        .iter()
        .find(|(_, i)| *i == 1)
        .map(|(r, _)| (r.x, r.y))
        .expect("choice hit");
    app.on_input(
        input::UiEvent::Mouse(input::MouseInput {
            kind: MouseEventKind::Down(MouseButton::Left),
            col: choice.0,
            row: choice.1,
            mods: Mods::NONE,
            sgr_position: None,
        }),
        120,
    );
    // Clicking a radio choice applies immediately (the reference's rule).
    assert_eq!(
        app.shell.toast_delivery,
        crate::config::ToastDelivery::Herdr
    );
    let outside = (0, 0);
    app.on_input(
        input::UiEvent::Mouse(input::MouseInput {
            kind: MouseEventKind::Down(MouseButton::Left),
            col: outside.0,
            row: outside.1,
            mods: Mods::NONE,
            sgr_position: None,
        }),
        120,
    );
    assert!(app.settings.is_none(), "an outside press cancels");
}

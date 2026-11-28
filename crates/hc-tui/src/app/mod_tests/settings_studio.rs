//! S0: the Settings overlay's `studio` tab — studio mode only, writes
//! to backlot's own `config.toml`, never herdr's.
use super::*;
use crate::chrome::settings::SettingsSection;
use crate::input::KeybindAction as A;
use crate::test_support::TestDir;
use hc_vt::{Action, Key, Mods};

fn key(app: &mut App, key: Key) {
    app.on_input(
        input::UiEvent::Key(term_key(key, Mods::NONE, Action::Press)),
        120,
    );
}

fn screen(app: &mut App) -> Vec<String> {
    screen_lines(&draw_to(app, 120, 40))
}

/// A temp backlot `config.toml` bound as `panel_config_path` — the only
/// file the studio tab may write.
fn backlot_file(app: &mut App, text: &str) -> TestDir {
    let dir = TestDir::new("hctui-backlot");
    app.panel_config_path = dir.join("config.toml");
    std::fs::write(&app.panel_config_path, text).unwrap();
    dir
}

#[tokio::test]
async fn the_studio_tab_appears_only_in_studio_mode() {
    // Studio mode: the tab is the last section.
    let mut app = studio::two_studio_app();
    let _root = backlot_file(&mut app, "");
    app.on_action(A::Settings, 120);
    let sections: Vec<_> = app
        .settings
        .as_ref()
        .map(|s| s.sections.clone())
        .unwrap_or_default();
    assert!(sections.contains(&SettingsSection::Studio));
    assert_eq!(sections.last(), Some(&SettingsSection::AgentSkill));
    assert!(
        app.settings
            .as_ref()
            .and_then(|s| s.studio.as_ref())
            .is_some()
    );
    let rendered = screen(&mut app).join("\n");
    assert!(rendered.contains("studio"), "{rendered}");

    // `--plain`: the tab is absent and the list is herdr's.
    let mut plain = super::app();
    plain.set_plain(true);
    plain.on_action(A::Settings, 120);
    let plain_sections: Vec<_> = plain
        .settings
        .as_ref()
        .map(|s| s.sections.clone())
        .unwrap_or_default();
    assert!(!plain_sections.contains(&SettingsSection::Studio));
    assert!(
        plain
            .settings
            .as_ref()
            .map(|s| s.studio.is_none())
            .unwrap_or(false)
    );
}

#[tokio::test]
async fn the_studio_tab_writes_start_alt_y_and_dock_width() {
    let mut app = studio::two_studio_app();
    let _root = backlot_file(&mut app, "# keep me\n[panel]\nkey = \"y\"\n");
    app.set_hollywood(Ok(None), &app.panel_config_path.clone());
    app.on_action(A::Settings, 120);
    app.select_settings_section(SettingsSection::Studio);
    let rendered = screen(&mut app).join("\n");
    assert!(rendered.contains("call sheet start"), "{rendered}");
    assert!(rendered.contains("collapsed"), "{rendered}");
    assert!(rendered.contains("direct alt+y"), "{rendered}");
    assert!(rendered.contains("dock width"), "{rendered}");
    assert!(rendered.contains("overseer"), "{rendered}");

    // Row 0 cycles the start policy: collapsed → when_waiting → expanded.
    key(&mut app, Key::Enter);
    assert_eq!(
        app.panel_config.call_sheet_start,
        crate::panel::CallSheetStart::WhenWaiting
    );
    let written = std::fs::read_to_string(&app.panel_config_path).unwrap();
    assert!(written.contains("[hollywood]"), "{written}");
    assert!(written.contains("start = \"when_waiting\""), "{written}");
    assert!(written.contains("# keep me"), "{written}");
    assert!(written.contains("[panel]"), "{written}");

    // Row 1 writes `[keys] call_sheet = ""` off, removes it back on.
    key(&mut app, Key::Char('j'));
    key(&mut app, Key::Enter);
    let written = std::fs::read_to_string(&app.panel_config_path).unwrap();
    assert!(written.contains("call_sheet = \"\""), "{written}");
    assert!(!app.hollywood_key.is_some_and(|k| k.direct.is_some()));
    key(&mut app, Key::Enter);
    let written = std::fs::read_to_string(&app.panel_config_path).unwrap();
    assert!(!written.contains("call_sheet"), "{written}");
    assert!(app.hollywood_key.is_some_and(|k| k.direct.is_some()));

    // Row 2 cycles the dock width 36 → 42 → 48 → 36.
    key(&mut app, Key::Char('j'));
    key(&mut app, Key::Enter);
    assert_eq!(app.panel_config.width, 42);
    let written = std::fs::read_to_string(&app.panel_config_path).unwrap();
    assert!(written.contains("[dock]"), "{written}");
    assert!(written.contains("width = 42"), "{written}");

    // The round-trip: the written file loads to the same values.
    let (loaded, notice) = crate::panel::load(&app.panel_config_path);
    assert!(notice.is_none(), "{notice:?}");
    assert_eq!(
        loaded.call_sheet_start,
        crate::panel::CallSheetStart::WhenWaiting
    );
    assert_eq!(loaded.width, 42);
    assert_eq!(loaded.keys_call_sheet, None);
}

/// R3-M15/R3-5: `Ctrl+B s` re-reads `[hollywood]` — an edit made
/// since start lands in the studio tab — the overseer row reads
/// `runner / model / effort` like the Call Sheet title (`effort`
/// drops when unset or `auto`), and Enter on it opens the setup
/// dialog.
#[tokio::test]
async fn the_overseer_row_re_reads_the_table_and_opens_setup() {
    let cfg = |effort: Option<&str>| backlot_stage::config::HollywoodCfg {
        runner: "fake".into(),
        model: "model-h".into(),
        effort: effort.map(str::to_string),
        key: None,
        default_cast: None,
    };
    let mut app = studio::two_studio_app();
    let _root = backlot_file(&mut app, "");
    // The label joins like the Call Sheet title.
    app.set_hollywood(Ok(Some(cfg(Some("high")))), &app.panel_config_path.clone());
    assert_eq!(
        app.hollywood_overseer.as_deref(),
        Some("fake / model-h / high")
    );
    app.set_hollywood(Ok(Some(cfg(Some("auto")))), &app.panel_config_path.clone());
    assert_eq!(app.hollywood_overseer.as_deref(), Some("fake / model-h"));
    app.set_hollywood(Ok(Some(cfg(None))), &app.panel_config_path.clone());
    assert_eq!(app.hollywood_overseer.as_deref(), Some("fake / model-h"));

    // The table lands after start — the open re-read picks it up
    // without a restart.
    app.set_hollywood(Ok(None), &app.panel_config_path.clone());
    assert_eq!(app.hollywood_overseer, None);
    // `effort = "auto"` parses (the runner rule needs it declared)
    // and the label drops it — the read lands as `fake / model-h`.
    std::fs::write(
        &app.panel_config_path,
        "[hollywood]\nrunner = \"fake\"\nmodel = \"model-h\"\neffort = \"auto\"\n",
    )
    .unwrap();
    app.on_action(A::Settings, 120);
    app.select_settings_section(SettingsSection::Studio);
    assert_eq!(app.hollywood_overseer.as_deref(), Some("fake / model-h"));
    assert_eq!(
        app.settings
            .as_ref()
            .and_then(|s| s.studio.as_ref())
            .and_then(|s| s.overseer.clone())
            .as_deref(),
        Some("fake / model-h"),
        "the row shows it"
    );

    // Enter on the overseer row opens the setup dialog — the probe
    // is stubbed like the dialog's own tests.
    app.set_ns_run(std::sync::Arc::new(move |_machine, argv, _stdin| {
        let out = if argv.first().map(String::as_str) == Some("runners") {
            Ok("[]".to_string())
        } else {
            Ok("[hollywood] written".to_string())
        };
        Box::pin(async move { out })
    }));
    for _ in 0..crate::chrome::settings::STUDIO_OVERSEER {
        key(&mut app, Key::Char('j'));
    }
    key(&mut app, Key::Enter);
    assert!(app.hw_switch.is_some(), "overseer Enter opens hw_switch");
}

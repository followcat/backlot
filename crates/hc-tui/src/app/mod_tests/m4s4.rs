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

// ---- Agent-menu operations (hc-tui extension) ----

use crate::chrome::menu::ContextMenuTarget;
use crate::testcore;
use ratatui::layout::Rect;
use serde_json::json;

fn pending(app: &mut App) {
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
}

/// Routes actions at a fake core answering as `core-fake-1`; the seen
/// worker shares the opener, one `rx.recv()` per connection.
fn operations(app: &mut App) -> mpsc::Receiver<testcore::FakeCore> {
    let (accepted, rx) = mpsc::channel(4);
    let (rpc, _sub) = testcore::openers(accepted);
    app.set_operations(rpc);
    app.view.core.as_mut().unwrap().id = "core-fake-1".into();
    rx
}

async fn settle(app: &mut App) {
    let result = tokio::time::timeout(
        testcore::DEADLINE,
        App::operation_result(&mut app.operations),
    )
    .await
    .unwrap();
    app.on_operation(result);
}

/// `t1` is a live agent with a turn; its sidebar row opens the menu.
fn app_agent() -> App {
    let mut app = app();
    let mut t1 = crate::sidebar::fixture::agent("t1", "working");
    t1.created_ms = 42;
    t1.turn = 5;
    t1.labels.insert("herdr.space".into(), "space-a".into());
    t1.labels.insert("herdr.tab".into(), "tab-a".into());
    // The core already recorded this turn as seen: the visible-pane share
    // pass stays quiet and only the explicit marks hit the wire.
    t1.labels.insert("herdr.seen".into(), "5".into());
    app.view = crate::sidebar::fixture::view(vec![t1], vec![]);
    pending(&mut app);
    app.reconcile_published_view();
    app.hit_map
        .agents
        .push((Rect::new(0, 0, 12, 1), "t1".into()));
    app
}

fn open_agent_menu(app: &mut App) {
    app.context_menu_press((1, 0));
    assert!(
        matches!(
            app.menu.as_ref().map(|m| &m.target),
            Some(ContextMenuTarget::Agent { pane_id }) if pane_id == "t1"
        ),
        "the agent row opens the agent menu"
    );
}

/// A `terminal.get` reply for `t1` at its fixture incarnation — the
/// result is the record itself.
fn get_reply() -> serde_json::Value {
    let mut term = testcore::minimal_terminal();
    term["address"] = json!("t1");
    term["created_ms"] = json!(42);
    term
}

/// Activate menu item `index` through the menu's real Enter path.
fn activate(app: &mut App, index: usize) {
    app.menu.as_mut().unwrap().highlighted = index;
    app.menu_key(&press(Key::Enter));
}

#[tokio::test]
async fn agent_right_click_opens_the_extension_menu_in_order() {
    let mut app = app_agent();
    open_agent_menu(&mut app);
    let labels: Vec<_> = app
        .menu
        .as_ref()
        .unwrap()
        .items()
        .iter()
        .map(|i| i.label)
        .collect();
    assert_eq!(
        labels,
        [
            "Send a line...",
            "Interrupt",
            "Mark read",
            "Mark unread",
            "Keep",
            "Park",
            "Kill",
            "Peek",
        ]
    );
}

#[tokio::test]
async fn agent_send_line_dialog_dispatches_terminal_send() {
    let mut app = app_agent();
    open_agent_menu(&mut app);
    let mut rx = operations(&mut app);
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.get", "terminal.send"], &[])
            .await;
        let get = core.read_request().await;
        assert_eq!(get["method"], "terminal.get");
        core.reply(&get["id"], get_reply()).await;
        let send = core.read_request().await;
        assert_eq!(send["method"], "terminal.send");
        assert_eq!(
            send["params"],
            json!({"terminal": "t1", "text": "fix the tests"})
        );
        core.reply(&send["id"], json!({})).await;
    });
    activate(&mut app, 0);
    let Some(dialog) = &app.dialog else {
        panic!("send a line opens the text dialog")
    };
    assert!(
        matches!(
            dialog.target,
            super::super::dialogs::Target::SendLine { .. }
        ),
        "the dialog carries the terminal target"
    );
    app.insert_dialog_text("fix the tests");
    app.dialog_key(&press(Key::Enter));
    settle(&mut app).await;
    assert!(app.notice.is_none());
    server.await.unwrap();
}

#[tokio::test]
async fn agent_interrupt_reaches_the_wire() {
    let mut app = app_agent();
    open_agent_menu(&mut app);
    let mut rx = operations(&mut app);
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.get", "terminal.interrupt"], &[])
            .await;
        let get = core.read_request().await;
        core.reply(&get["id"], get_reply()).await;
        let interrupt = core.read_request().await;
        assert_eq!(interrupt["method"], "terminal.interrupt");
        assert_eq!(interrupt["params"], json!({"terminal": "t1"}));
        core.reply(&interrupt["id"], json!({})).await;
    });
    activate(&mut app, 1);
    settle(&mut app).await;
    assert!(app.notice.is_none());
    server.await.unwrap();
}

#[tokio::test]
async fn agent_mark_read_writes_the_seen_turn() {
    let mut app = app_agent();
    open_agent_menu(&mut app);
    let mut rx = operations(&mut app);
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.label"], &[]).await;
        let write = core.read_request().await;
        assert_eq!(write["method"], "terminal.label");
        assert_eq!(
            write["params"],
            json!({"terminal": "t1", "labels": {"herdr.seen": "5"}})
        );
        core.reply(&write["id"], json!({})).await;
    });
    activate(&mut app, 2);
    server.await.unwrap();
    assert_eq!(app.seen_looked.get("t1"), Some(&5));
}

#[tokio::test]
async fn agent_mark_unread_lowers_the_turn_and_zero_is_inert() {
    let mut app = app_agent();
    open_agent_menu(&mut app);
    let mut rx = operations(&mut app);
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.label"], &[]).await;
        let write = core.read_request().await;
        assert_eq!(
            write["params"],
            json!({"terminal": "t1", "labels": {"herdr.seen": "4"}})
        );
        core.reply(&write["id"], json!({})).await;
    });
    activate(&mut app, 3);
    server.await.unwrap();
    // Turn 0: nothing has finished, so there is nothing to mark.
    let mut zero = super::app();
    let mut t0 = crate::sidebar::fixture::agent("t0", "working");
    t0.turn = 0;
    zero.view = crate::sidebar::fixture::view(vec![t0], vec![]);
    zero.reconcile_published_view();
    zero.mark_unseen_for("t0");
    assert!(zero.seen_inflight.is_empty());
}

#[tokio::test]
async fn agent_keep_and_park_reach_the_wire() {
    let mut app = app_agent();
    open_agent_menu(&mut app);
    let mut rx = operations(&mut app);
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(
            true,
            &["terminal.get", "terminal.keep", "terminal.label"],
            &[],
        )
        .await;
        for expected in ["get", "keep", "get", "label", "keep"] {
            let req = core.read_request().await;
            assert_eq!(req["method"], format!("terminal.{expected}"));
            let reply = if req["method"] == "terminal.get" {
                get_reply()
            } else {
                json!({})
            };
            core.reply(&req["id"], reply).await;
        }
    });
    // Keep: terminal.get verifies, then keep=true.
    activate(&mut app, 4);
    settle(&mut app).await;
    // Park: incarnation check, label clear, then keep=false.
    open_agent_menu(&mut app);
    activate(&mut app, 5);
    settle(&mut app).await;
    server.await.unwrap();
}

#[tokio::test]
async fn agent_park_writes_herdr_labels_then_keep_false() {
    let mut app = app_agent();
    open_agent_menu(&mut app);
    let mut rx = operations(&mut app);
    let (check_tx, mut check) = mpsc::channel(4);
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(
            true,
            &["terminal.get", "terminal.keep", "terminal.label"],
            &[],
        )
        .await;
        let get = core.read_request().await;
        core.reply(&get["id"], get_reply()).await;
        let label = core.read_request().await;
        check_tx.send(label["params"].clone()).await.unwrap();
        core.reply(&label["id"], json!({})).await;
        let keep = core.read_request().await;
        check_tx.send(keep["params"].clone()).await.unwrap();
        core.reply(&keep["id"], json!({})).await;
    });
    activate(&mut app, 5);
    settle(&mut app).await;
    server.await.unwrap();
    let label = check.recv().await.unwrap();
    assert_eq!(
        label,
        json!({"terminal": "t1", "labels": {"herdr.space": "", "herdr.tab": ""}})
    );
    let keep = check.recv().await.unwrap();
    assert_eq!(keep, json!({"terminal": "t1", "keep": false}));
}

#[tokio::test]
async fn agent_kill_requires_the_confirmation() {
    let mut app = app_agent();
    // Wire the fake core first: the dialog stamps the core id at open
    // time and `save_dialog` refuses a mismatch.
    let mut rx = operations(&mut app);
    open_agent_menu(&mut app);
    activate(&mut app, 6);
    let Some(dialog) = &app.dialog else {
        panic!("kill opens the confirmation")
    };
    assert!(matches!(
        dialog.target,
        super::super::dialogs::Target::AgentKill { .. }
    ));
    // Esc cancels — nothing reaches the wire. (The reference's confirm
    // Esc also leaves Navigate mode active; that doesn't matter here.)
    app.dialog_key(&press(Key::Escape));
    assert!(app.dialog.is_none());
    assert_eq!(app.pending_actions, 0);
    // Confirm: terminal.get verifies, then terminal.kill.
    open_agent_menu(&mut app);
    activate(&mut app, 6);
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.get", "terminal.kill"], &[])
            .await;
        let get = core.read_request().await;
        core.reply(&get["id"], get_reply()).await;
        let kill = core.read_request().await;
        assert_eq!(kill["method"], "terminal.kill");
        assert_eq!(kill["params"], json!({"terminal": "t1"}));
        core.reply(&kill["id"], get_reply()).await;
    });
    app.dialog_key(&press(Key::Enter));
    settle(&mut app).await;
    assert!(app.notice.is_none());
    server.await.unwrap();
}

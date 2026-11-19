use super::*;
use crate::test_support::TestDir;

fn scratch() -> TestDir {
    TestDir::new("hctui-prefs")
}

fn shell_with_preferences(path: &std::path::Path) -> ClientShellConfig {
    let config = crate::config::Config::default();
    crate::chrome::ClientShellConfig::from_config(&config, None, None, std::path::PathBuf::new())
        .with_preferences_path(path.to_path_buf())
}

fn app_with_shell(shell: ClientShellConfig) -> App {
    let (accepted, _rx) = mpsc::channel(1);
    let (rpc, sub) = crate::testcore::openers(accepted);
    let watcher = Watcher::with_openers(
        rpc,
        sub,
        hc_proto::watch::WatchParams {
            traversal: Some(crate::project::agent_traversal),
            ..hc_proto::watch::WatchParams::default()
        },
    );
    let open: OpenAttach = Arc::new(|_| unreachable!("no attach in this test"));
    let config = crate::config::Config::default();
    let live = config.live_keybinds();
    App::new(watcher, open, live, shell, Arc::new(Probe::default()))
}

#[tokio::test]
async fn agent_panel_sort_defaults_to_priority() {
    assert_eq!(
        app().shell.agent_panel_sort,
        crate::config::AgentPanelSortConfig::Priority
    );
}

#[tokio::test]
async fn clicked_sort_persists_and_a_fresh_app_remembers_it() {
    let root = scratch();
    let path = root.join("prefs.json");
    let mut app = app_with_shell(shell_with_preferences(&path));
    app.flip_agent_sort();
    assert_eq!(
        app.shell.agent_panel_sort,
        crate::config::AgentPanelSortConfig::Spaces
    );
    let saved = crate::preferences::load(&path).expect("click writes the file");
    assert_eq!(
        saved.agent_panel_sort,
        Some(crate::config::AgentPanelSortConfig::Spaces)
    );

    // A new App on the same endpoint starts with the clicked order and
    // treats it as a manual choice.
    let restarted = app_with_shell(shell_with_preferences(&path));
    assert_eq!(
        restarted.shell.agent_panel_sort,
        crate::config::AgentPanelSortConfig::Spaces
    );
    assert!(restarted.agent_panel_sort_manual);

    // A different endpoint's file is untouched: it starts at the default.
    let other = app_with_shell(shell_with_preferences(&root.join("other.json")));
    assert_eq!(
        other.shell.agent_panel_sort,
        crate::config::AgentPanelSortConfig::Priority
    );
    assert!(!other.agent_panel_sort_manual);
}

#[tokio::test]
async fn a_saved_choice_wins_over_the_config_at_start() {
    let root = scratch();
    let path = root.join("prefs.json");
    std::fs::write(&path, r#"{"agent_panel_sort":"spaces"}"#).unwrap();
    let app = app_with_shell(shell_with_preferences(&path));
    assert_eq!(
        app.shell.agent_panel_sort,
        crate::config::AgentPanelSortConfig::Spaces
    );
    assert!(app.agent_panel_sort_manual);
}

#[tokio::test]
async fn malformed_or_missing_preferences_fall_back_to_config() {
    let root = scratch();
    let path = root.join("prefs.json");
    std::fs::write(&path, "not json {").unwrap();
    let app = app_with_shell(shell_with_preferences(&path));
    assert_eq!(
        app.shell.agent_panel_sort,
        crate::config::AgentPanelSortConfig::Priority
    );
    let missing = app_with_shell(shell_with_preferences(&root.join("missing.json")));
    assert_eq!(
        missing.shell.agent_panel_sort,
        crate::config::AgentPanelSortConfig::Priority
    );
}

#[tokio::test]
async fn sidebar_toggle_persists_the_collapsed_choice() {
    let root = scratch();
    let path = root.join("prefs.json");
    let mut app = app_with_shell(shell_with_preferences(&path));
    app.on_action(crate::input::KeybindAction::ToggleSidebar, 120);
    assert!(app.shell_state.sidebar_collapsed);
    assert!(app.sidebar_collapsed_manual);
    let saved = crate::preferences::load(&path).expect("toggle writes the file");
    assert_eq!(saved.sidebar_collapsed, Some(true));

    let restarted = app_with_shell(shell_with_preferences(&path));
    assert!(restarted.shell_state.sidebar_collapsed);
    assert!(restarted.sidebar_collapsed_manual);
}

#[tokio::test]
async fn sidebar_width_drag_persists_on_release() {
    let root = scratch();
    let path = root.join("prefs.json");
    let mut app = app_with_shell(shell_with_preferences(&path));
    app.chrome_drag = Some(super::super::mouse::ChromeDrag::SidebarWidth);
    app.set_sidebar_width_from_column(29);
    app.mouse_release((30, 5));
    assert_eq!(app.sidebar_width, 30);
    let saved = crate::preferences::load(&path).expect("release writes the file");
    assert_eq!(saved.sidebar_width, Some(30));

    let restarted = app_with_shell(shell_with_preferences(&path));
    assert_eq!(restarted.sidebar_width, 30);
    assert!(restarted.sidebar_width_manual);
}

#[tokio::test]
async fn group_collapse_persists() {
    let root = scratch();
    let path = root.join("prefs.json");
    let mut app = app_with_shell(shell_with_preferences(&path));
    app.toggle_collapsed_group(
        &crate::chrome::snapshot::ClientEndpointId::Local,
        "acme-shop".to_owned(),
    );
    let saved = crate::preferences::load(&path).expect("collapse writes the file");
    assert_eq!(saved.collapsed_groups, vec!["acme-shop".to_owned()]);

    let restarted = app_with_shell(shell_with_preferences(&path));
    assert!(restarted.shell_state.collapsed_groups.contains("acme-shop"));
}

#[tokio::test]
async fn reload_keeps_the_remembered_sort_over_a_new_config_value() {
    let root = scratch();
    let path = root.join("prefs.json");
    std::fs::write(&path, r#"{"agent_panel_sort":"spaces"}"#).unwrap();
    let mut app = app_with_shell(shell_with_preferences(&path));
    assert_eq!(
        app.shell.agent_panel_sort,
        crate::config::AgentPanelSortConfig::Spaces
    );
    // The config on disk now says priority; a reload keeps the remembered
    // manual choice instead of applying the file.
    let config_path = root.join("config.toml");
    std::fs::write(&config_path, "[ui]\nagent_panel_sort='priority'\n").unwrap();
    app.shell.local_config_path = config_path;
    app.on_action(crate::input::KeybindAction::ReloadConfig, 120);
    assert_eq!(
        app.shell.agent_panel_sort,
        crate::config::AgentPanelSortConfig::Spaces
    );
}

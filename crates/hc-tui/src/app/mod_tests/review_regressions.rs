use super::*;

#[tokio::test]
async fn armed_prefix_binding_wins_over_colliding_direct_chord() {
    let mut app = app();
    let config: crate::config::Config =
        toml::from_str("[keys]\ntoggle_sidebar='ctrl+x'\nhelp='prefix+ctrl+x'\n").unwrap();
    app.live = config.live_keybinds();
    app.on_key(
        term_key(
            hc_vt::Key::Char('b'),
            hc_vt::Mods::CTRL,
            hc_vt::Action::Press,
        ),
        120,
    );
    assert!(app.prefix.armed());
    app.on_key(
        term_key(
            hc_vt::Key::Char('x'),
            hc_vt::Mods::CTRL,
            hc_vt::Action::Press,
        ),
        120,
    );
    assert!(app.help);
    assert!(!app.prefix.armed());
    assert!(!app.shell_state.sidebar_collapsed);
}

#[tokio::test]
async fn pasted_help_query_starts_at_the_first_result() {
    let mut app = app();
    app.help = true;
    app.help_state.search_focused = true;
    app.help_state.scroll = 10;
    app.on_input(UiEvent::Paste(b"pane".to_vec()), 120);
    assert_eq!(app.help_state.scroll, 0);
    app.help_state.scroll = 10;
    app.on_input(UiEvent::Text(" tab".into()), 120);
    assert_eq!(app.help_state.scroll, 0);
}

#[tokio::test]
async fn configured_tab_text_keeps_literal_spacing_and_filters_controls() {
    let mut app = app();
    app.shell.tab_bar_right = vec![crate::config::TabBarRightEntryConfig::Text {
        text: "  fixture\x1b \n".into(),
    }];
    app.shell.tab_bar_right_separator = " |\n ".into();
    app.update_tab_status(Instant::now());
    assert_eq!(app.projection.snapshot.tab_bar_right[0].text, "  fixture ");
    assert_eq!(app.projection.snapshot.tab_bar_right_separator, " | ");
    assert!(app.next_tab_status.is_none());
}

#[tokio::test]
async fn outer_focus_loss_cancels_an_offline_undispatched_key_tap() {
    let mut app = app();
    let (sender, receiver) = mpsc::channel(64);
    app.pane_rx = receiver;
    app.pane = crate::app::pane::Pane::new(sender, Arc::new(|_| Box::pin(std::future::pending())));
    app.pane.want(Some(Spec {
        terminal: "t7".into(),
        role: Role::Active,
    }));
    let generation = app.pane.generation();
    app.pane.on_event(
        PaneEv::Failed {
            generation,
            why: "disconnected".into(),
            unsupported: false,
        },
        Instant::now(),
    );
    app.on_key(
        term_key(hc_vt::Key::ArrowUp, hc_vt::Mods::NONE, hc_vt::Action::Press),
        120,
    );
    assert_eq!(app.pane.input_backlog().0, 1);
    app.on_input(UiEvent::Focus(false), 120);
    app.on_key(
        term_key(
            hc_vt::Key::ArrowUp,
            hc_vt::Mods::NONE,
            hc_vt::Action::Release,
        ),
        120,
    );
    assert_eq!(app.pane.input_backlog().0, 0);
}

#[tokio::test]
async fn datetime_status_uses_source_trimming_and_format_control_filtering() {
    let mut app = app();
    app.shell.tab_bar_right = vec![crate::config::TabBarRightEntryConfig::Datetime {
        format: "  fixture\u{200b}  ".into(),
    }];
    app.update_tab_status(Instant::now());
    assert_eq!(app.projection.snapshot.tab_bar_right[0].text, "fixture");
    assert!(app.next_tab_status.is_some());
}

#[tokio::test]
async fn armed_prefix_ignores_clipboard_commits() {
    let mut app = app();
    let (sender, receiver) = mpsc::channel(64);
    app.pane_rx = receiver;
    app.pane = crate::app::pane::Pane::new(sender, Arc::new(|_| Box::pin(std::future::pending())));
    app.pane.want(Some(Spec {
        terminal: "t7".into(),
        role: Role::Active,
    }));
    app.on_key(
        term_key(
            hc_vt::Key::Char('b'),
            hc_vt::Mods::CTRL,
            hc_vt::Action::Press,
        ),
        120,
    );
    app.on_input(UiEvent::Paste(b"literal".to_vec()), 120);
    app.on_input(UiEvent::Text("commit".into()), 120);
    assert!(app.prefix.armed());
    assert_eq!(app.pane.input_backlog().0, 0);
}

#[tokio::test]
async fn a_rejected_press_cannot_create_a_forwarded_release_lease() {
    let mut app = app();
    let (sender, receiver) = mpsc::channel(64);
    app.pane_rx = receiver;
    app.pane = crate::app::pane::Pane::new(sender, Arc::new(|_| Box::pin(std::future::pending())));
    app.pane.want(Some(Spec {
        terminal: "t7".into(),
        role: Role::Active,
    }));
    for _ in 0..1024 {
        app.pane.paste(b"x");
    }
    app.on_key(
        term_key(hc_vt::Key::ArrowUp, hc_vt::Mods::NONE, hc_vt::Action::Press),
        120,
    );
    assert_eq!(app.pane_leases.forwarded().count(), 0);
}

//! Focused-panel keys: movement, flipping, group collapse, the Enter
//! actions (focus, two-step reopen, non-resumable refusal) and the
//! injected `priority` runner with its failure notice.
use super::*;
use crate::panel::Target;

#[tokio::test]
async fn focused_panel_keys_move_flip_collapse_and_do_not_leak() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    // j selects the next row (title → studio), k back.
    key(&mut app, hc_vt::Key::Char('j'));
    assert_eq!(app.panel.selected, Some(Target::Studio("acme-shop".into())));
    key(&mut app, hc_vt::Key::Char('k'));
    assert_eq!(app.panel.selected, Some(Target::Title));
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.panel.selected, Some(Target::Studio("acme-shop".into())));
    key(&mut app, hc_vt::Key::ArrowUp);
    // PageDown leaps a viewport — the hit map comes from a real draw.
    draw_to(&mut app, 120, 24);
    key(&mut app, hc_vt::Key::PageDown);
    assert!(matches!(
        app.panel.selected,
        Some(Target::Group(..) | Target::Person(..))
    ));
    // c collapses the panel to its strip.
    key(&mut app, hc_vt::Key::Char('c'));
    assert_eq!(app.panel.mode, PanelMode::Collapsed);
    key(&mut app, hc_vt::Key::Char('c'));
    assert_eq!(app.panel.mode, PanelMode::Expanded);
    // Nothing reached the terminal.
    assert_eq!(app.pane.input_backlog().0, 0, "focused keys leaked");
}

#[tokio::test]
async fn space_collapses_the_selected_group() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Group("acme-shop".into(), "actors".into()));
    key(&mut app, hc_vt::Key::Char(' '));
    assert!(
        app.panel
            .collapsed_groups
            .contains(&("acme-shop".into(), "actors".into()))
    );
    key(&mut app, hc_vt::Key::Char(' '));
    assert!(app.panel.collapsed_groups.is_empty());
}

#[tokio::test]
async fn enter_on_a_person_focuses_their_terminal() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(app.focused.as_deref(), Some("t4"));
    assert!(!app.panel.focused);
    assert_eq!(app.panel.mode, PanelMode::Expanded, "beside stays open");
}

#[tokio::test]
async fn a_full_screen_panel_owns_the_keyboard() {
    let mut app = app_with_panel_doc();
    app.area = (40, 20);
    open_panel(&mut app);
    assert!(app.panel_fullscreen());
    assert!(app.panel_focused());
    // Even with the focus latch off the panel still eats the keys —
    // nobody can see the panes it covers.
    app.panel.focused = false;
    assert!(app.panel_focused());
    key(&mut app, hc_vt::Key::Char('j'));
    assert_eq!(app.panel.selected, Some(Target::Studio("acme-shop".into())));
    assert_eq!(app.pane.input_backlog().0, 0, "keys leaked to the pane");
}

#[tokio::test]
async fn esc_closes_a_full_screen_panel() {
    let mut app = app_with_panel_doc();
    app.area = (40, 20);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.panel.mode, PanelMode::Closed);
    // Beside the panes Esc only leaves focus, panel stays open.
    app.area = (200, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.panel.focused);
    assert_eq!(app.panel.mode, PanelMode::Expanded);
}

#[tokio::test]
async fn the_chord_closes_full_screen_on_the_first_press() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    // Unfocus beside the panes, then shrink under the threshold: the
    // next chord must close, not re-focus.
    key(&mut app, hc_vt::Key::Escape);
    app.area = (40, 20);
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(app.panel.mode, PanelMode::Closed);
}

#[tokio::test]
async fn enter_on_a_person_closes_a_full_screen_panel() {
    let mut app = app_with_panel_doc();
    app.area = (40, 20);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(app.focused.as_deref(), Some("t4"));
    assert_eq!(app.panel.mode, PanelMode::Closed);
}

#[tokio::test]
async fn esc_disarms_a_pending_reopen() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-dave".into()));
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.panel.reopen_confirm.is_some());
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.panel.reopen_confirm.is_none(), "Esc disarms the reopen");
    // Esc also left focus; the chord re-focuses and dave's next Enter
    // starts the two-step over instead of reopening.
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('y'));
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        app.panel.reopen_confirm,
        Some(("acme-shop".into(), "actor-dave".into()))
    );
}

#[tokio::test]
async fn a_bad_panel_key_still_toggles_with_y() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    let dir = std::env::temp_dir().join(format!("panel-key-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "[panel]\nkey = \"nope+\"\n").unwrap();
    let (config, notice) = crate::panel::load(&path);
    app.set_panel_config(config, notice);
    assert!(app.notice.is_some(), "the bad key notices once");
    // The notice names the file, never the bogus value — the screen
    // shows no "nope".
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(!text.contains("nope"), "{text}");
    // The combo fell back to `y` — the chord still toggles.
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(app.panel.mode, PanelMode::Expanded);
}

#[tokio::test]
async fn activating_another_row_disarms_a_pending_reopen() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-dave".into()));
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.panel.reopen_confirm.is_some());
    // Enter on a different person disarms dave's armed row.
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.panel.reopen_confirm.is_none());
}

#[tokio::test]
async fn enter_on_a_studio_focuses_its_producer() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Studio("acme-shop".into()));
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(app.focused.as_deref(), Some("t1"));
    assert!(!app.panel.focused);
}

#[tokio::test]
async fn a_closed_person_reopens_on_the_second_enter() {
    let mut app = app_with_panel_doc();
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_panel_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(()) })
    }));
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-dave".into()));
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        app.panel.reopen_confirm,
        Some(("acme-shop".into(), "actor-dave".into()))
    );
    assert!(recorded.lock().unwrap().is_empty(), "first Enter only arms");
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.panel.reopen_confirm.is_none());
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec![
            "reopen".to_string(),
            "--person".into(),
            "actor-dave".into(),
            "--studio".into(),
            "acme-shop".into(),
            "--as".into(),
            "user".into(),
        ]]
    );
}

#[tokio::test]
async fn a_non_resumable_person_says_why_instead_of_reopening() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-eve".into()));
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.panel.reopen_confirm.is_none());
    assert!(app.notice.is_some(), "the refusal notices");
}

#[tokio::test]
async fn plus_and_minus_raise_and_lower_an_actors_priority() {
    let mut app = app_with_panel_doc();
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_panel_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(()) })
    }));
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    key(&mut app, hc_vt::Key::Char('+'));
    key(&mut app, hc_vt::Key::Char('-'));
    let argv = recorded.lock().unwrap();
    assert_eq!(argv.len(), 2);
    // `priority --task task-1 --level high|low --studio acme-shop --as user`
    assert_eq!(argv[0][..2], ["priority", "--task"]);
    assert_eq!(argv[0][2], "task-1");
    assert_eq!(argv[0][4], "high");
    // The second press reads the document's (still `normal`) level —
    // the stage has not reported the raise back yet.
    assert_eq!(argv[1][4], "low");
    assert_eq!(argv[0][6], "acme-shop");
    // Bob is already low — minus there is a no-op.
    drop(argv);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-bob".into()));
    key(&mut app, hc_vt::Key::Char('-'));
    assert_eq!(recorded.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn a_failed_action_shows_its_first_stderr_line() {
    let mut app = app_with_panel_doc();
    app.on_panel_action(("priority alice high".into(), Err("studio went away".into())));
    assert!(app.notice.is_some());
    let backend = draw_to(&mut app, 120, 40);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("studio went away"), "{text}");
}

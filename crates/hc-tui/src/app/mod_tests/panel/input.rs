//! The `›` line's keys: `i`/`/` select it (the derived editing
//! does the rest), a plain line is a `tell` to the producer, a
//! `/verb` dispatches through the studio-menu arm — `/stop` arms
//! the same confirm `m` would — and an armed `finish` prompt's
//! letters answer it instead of landing in the draft.
use super::*;

/// A `terminal.run`-recording panel runner — the local dock's
/// `backlot …` argv capture.
fn runner() -> (
    std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    crate::app::action_tracker::TestPanelRunner,
) {
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    (
        recorded,
        std::sync::Arc::new(move |argv: Vec<String>| {
            sink.lock().unwrap().push(argv);
            Box::pin(async { Ok(()) })
        }),
    )
}

#[tokio::test]
async fn i_lands_the_selection_on_the_input_row_and_types() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('i'));
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    assert!(app.panel_editing());
    // Letters are text on the line, not dock keys — `j` moves
    // nothing while the line owns the keys.
    key(&mut app, hc_vt::Key::Char('j'));
    assert_eq!(app.panel.input.as_str(), "j");
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
}

#[tokio::test]
async fn a_plain_line_tells_the_producer_and_the_line_stays() {
    let mut app = app_with_panel_doc();
    let (recorded, run) = runner();
    app.set_panel_runner(run);
    app.area = (120, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('i'));
    for ch in "check the queue".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec![
            "tell".to_string(),
            "producer".into(),
            "--text".into(),
            "check the queue".into(),
            "--studio".into(),
            "acme-shop".into(),
            "--as".into(),
            "user".into(),
        ]]
    );
    // The buffer cleared but the line stays selected — the next
    // line sends at once (the Call Sheet's keep-talking rule).
    assert_eq!(app.panel.input.as_str(), "");
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    assert!(app.panel_editing());
}

#[tokio::test]
async fn esc_and_up_step_back_into_the_rows() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('i'));
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.panel_editing());
    assert!(matches!(
        app.panel.selected,
        Some(Target::Person(..) | Target::Studio(..) | Target::Task(..) | Target::Fold(..))
    ));
    // ↑ from the line does the same step.
    key(&mut app, hc_vt::Key::Char('i'));
    key(&mut app, hc_vt::Key::Char('x'));
    key(&mut app, hc_vt::Key::ArrowUp);
    assert!(!app.panel_editing());
    // …and ↓ steps back onto the line.
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    assert!(app.panel_editing());
}

#[tokio::test]
async fn i_on_a_person_seeds_their_tell() {
    let mut app = app_with_panel_doc();
    let (recorded, run) = runner();
    app.set_panel_runner(run);
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    key(&mut app, hc_vt::Key::Char('i'));
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    assert_eq!(app.panel.input.as_str(), "/tell actor-alice ");
    // The press named its target — finishing the line sends
    // `tell actor-alice` without touching the dialog.
    for ch in "ship it".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec![
            "tell".to_string(),
            "actor-alice".into(),
            "--text".into(),
            "ship it".into(),
            "--studio".into(),
            "acme-shop".into(),
            "--as".into(),
            "user".into(),
        ]]
    );
}

#[tokio::test]
async fn slash_stop_arms_the_same_confirm_the_menu_arms() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('/'));
    assert_eq!(app.panel.input.as_str(), "/");
    for ch in "stop".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    // The deterministic verb opened `m`'s confirm — no LLM, no
    // action run yet (the dialog decides).
    assert!(
        matches!(
            app.studio_dialog,
            Some(crate::studio_dialog::StudioDialog::Confirm(_))
        ),
        "{:?}",
        app.studio_dialog.is_some()
    );
}

#[tokio::test]
async fn an_unknown_verb_notices_with_the_verb_list() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('/'));
    for ch in "bogus".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.notice.is_some(), "a bad verb notices");
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains("/bogus"), "{text}");
    assert!(text.contains("/stop"), "{text}");
}

#[tokio::test]
async fn an_armed_finish_prompt_answers_instead_of_drafting() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('i'));
    // The `finish <id>? [y/n]` the `f` key arms outranks the line's
    // edit — `y` decides, it never lands as text.
    app.panel.finish_confirm = Some("acme-shop".into());
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(app.panel.input.as_str(), "");
}

//! Card actions and the input line through the injected runner:
//! exact argv for Apply / Cancel / Show me / `tell hollywood`, the
//! "Show me" read-only view, studio focus for other cards and the
//! failure notice.
use super::*;

#[tokio::test]
async fn apply_and_cancel_run_the_exact_argv() {
    let mut app = app_with_hollywood();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);
    alt_y(&mut app);
    app.hollywood.selected = Some(Target::Card("c3".into()));
    key(&mut app, hc_vt::Key::Char('a'));
    key(&mut app, hc_vt::Key::Char('c'));
    let argv = recorded.lock().unwrap().clone();
    assert_eq!(
        argv,
        vec![
            vec!["card", "apply", "c3", "--as", "user"],
            vec![
                "card",
                "cancel",
                "c3",
                "--reason",
                "cancelled from the layer",
                "--as",
                "user"
            ],
        ]
    );
}

#[tokio::test]
async fn show_me_on_a_studio_new_card_runs_card_show_and_opens_the_view() {
    let mut app = app_with_hollywood();
    let (recorded, _) = recorder();
    let sink = recorded.clone();
    app.set_hollywood_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok("cast: alice\nbrief: build the shop".to_string()) })
    }));
    app.area = (120, 24);
    alt_y(&mut app);
    app.hollywood.selected = Some(Target::Card("c4".into()));
    key(&mut app, hc_vt::Key::Char('s'));
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec!["card", "show", "c4", "--as", "user"]]
    );
    // The runner's reply lands as the read-only view.
    let result = app.hwood_rx.recv().await.expect("the action's reply");
    app.on_hollywood_action(result);
    let view = app.hollywood.view.as_ref().expect("the Show me view");
    assert_eq!(view.title, "c4");
    assert!(view.lines.iter().any(|l| l.contains("brief")));
    // The view owns keys until it closes; Esc leaves it.
    key(&mut app, hc_vt::Key::Char('j'));
    assert_eq!(app.hollywood.view.as_ref().unwrap().scroll, 1);
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.hollywood.view.is_none());
}

#[tokio::test]
async fn show_me_on_any_other_card_focuses_its_studio() {
    let mut app = app_with_hollywood();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);
    alt_y(&mut app);
    app.hollywood.selected = Some(Target::Card("c3".into()));
    key(&mut app, hc_vt::Key::Char('s'));
    assert!(recorded.lock().unwrap().is_empty(), "no command ran");
    assert_eq!(app.focused.as_deref(), Some("t1"), "the producer focused");
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
}

#[tokio::test]
async fn enter_on_the_title_focuses_bl_hollywood() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    alt_y(&mut app);
    app.hollywood.selected = Some(Target::Title);
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(app.focused.as_deref(), Some("t10"));
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
}

#[tokio::test]
async fn enter_on_a_studio_row_focuses_its_producer() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    alt_y(&mut app);
    app.hollywood.selected = Some(Target::Studio("acme-shop".into()));
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(app.focused.as_deref(), Some("t1"));
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
}

#[tokio::test]
async fn the_input_line_sends_tell_hollywood_and_empty_sends_nothing() {
    let mut app = app_with_hollywood();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);
    alt_y(&mut app);
    key(&mut app, hc_vt::Key::Char('i'));
    // Empty input: Enter sends nothing and still leaves the line.
    key(&mut app, hc_vt::Key::Enter);
    assert!(recorded.lock().unwrap().is_empty());
    assert!(!app.hollywood.editing);
    key(&mut app, hc_vt::Key::Char('i'));
    key(&mut app, hc_vt::Key::Char('h'));
    key(&mut app, hc_vt::Key::Char('i'));
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec!["tell", "hollywood", "hi", "--as", "user"]]
    );
    assert_eq!(app.hollywood.input.as_str(), "");
    // Enter sent and left the line — `x` is the layer's hide key
    // again, not an editor character.
    key(&mut app, hc_vt::Key::Char('x'));
    assert_eq!(app.hollywood.mode, LayerMode::Hidden);
}

#[tokio::test]
async fn a_failed_action_shows_a_notice_with_the_stderr_line() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    app.on_hollywood_action(("card apply c3".into(), Err("no card 'c3'".into()), false));
    assert!(app.notice.is_some());
    let text = screen_lines(&draw_to(&mut app, 120, 40)).join("\n");
    assert!(text.contains("no card 'c3'"), "{text}");
}

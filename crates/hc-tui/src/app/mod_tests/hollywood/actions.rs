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
    key(&mut app, hc_vt::Key::Char('x'));
    assert!(app.studio_dialog.is_some(), "the cancel reason dialog");
    key(&mut app, hc_vt::Key::Enter);
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
                "cancelled from the call sheet",
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
    let result = app.action_rx.recv().await.expect("the action's reply");
    app.on_action_reply(result);
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
async fn the_view_applies_and_cancels_the_card_it_shows() {
    // S1 §4: inside the view `a` applies and `x` cancels the card —
    // the same argv and feedback as on the row; `x` is no longer a
    // close.
    let mut app = app_with_hollywood();
    let (recorded, _) = recorder();
    let sink = recorded.clone();
    app.set_hollywood_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok("cast: alice".to_string()) })
    }));
    app.area = (120, 24);
    alt_y(&mut app);
    app.hollywood.selected = Some(Target::Card("c3".into()));
    key(&mut app, hc_vt::Key::Char('s'));
    let result = app.action_rx.recv().await.expect("the view");
    app.on_action_reply(result);
    assert!(app.hollywood.view.is_some());
    key(&mut app, hc_vt::Key::Char('a'));
    key(&mut app, hc_vt::Key::Char('x'));
    assert!(app.studio_dialog.is_some(), "the cancel reason dialog");
    key(&mut app, hc_vt::Key::Enter);
    let argv = recorded.lock().unwrap().clone();
    assert_eq!(
        argv,
        vec![
            vec!["card", "show", "c3", "--as", "user"],
            vec!["card", "apply", "c3", "--as", "user"],
            vec![
                "card",
                "cancel",
                "c3",
                "--reason",
                "cancelled from the call sheet",
                "--as",
                "user"
            ],
        ]
    );
    // The view is still open — it owns keys until Esc/Enter/outside.
    assert!(app.hollywood.view.is_some());
}

#[tokio::test]
async fn a_card_answer_from_the_view_closes_it_on_success() {
    let mut app = app_with_hollywood();
    let (recorded, _) = recorder();
    let sink = recorded.clone();
    app.set_hollywood_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(String::new()) })
    }));
    app.area = (120, 24);
    app.hollywood.view = Some(crate::hollywood::CardView {
        card: "c3".into(),
        title: "c3".into(),
        lines: vec!["cast: alice".into()],
        scroll: 0,
    });
    app.hollywood.mode = LayerMode::Expanded;
    key(&mut app, hc_vt::Key::Char('a'));
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec!["card", "apply", "c3", "--as", "user"]]
    );
    let result = app.action_rx.recv().await.expect("the result");
    app.on_action_reply(result);
    assert!(
        app.hollywood.view.is_none(),
        "a resolved card's read retires with the row"
    );
}

#[tokio::test]
async fn a_failed_card_answer_keeps_the_view_and_notices() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    app.hollywood.view = Some(crate::hollywood::CardView {
        card: "c3".into(),
        title: "c3".into(),
        lines: vec!["cast: alice".into()],
        scroll: 0,
    });
    app.hollywood.mode = LayerMode::Expanded;
    app.on_hollywood_action(("card apply c3".into(), Err("no card 'c3'".into()), false));
    assert!(app.hollywood.view.is_some(), "the read stays on a failure");
    assert!(app.action_toast.is_some());
    // And Enter closes it.
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.hollywood.view.is_none());
}

#[tokio::test]
async fn show_me_runs_card_show_on_any_card() {
    // S1: `s` shows any card's detail, not only a studio-new draft.
    let mut app = app_with_hollywood();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);
    alt_y(&mut app);
    app.hollywood.selected = Some(Target::Card("c3".into()));
    key(&mut app, hc_vt::Key::Char('s'));
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec!["card", "show", "c3", "--as", "user"]]
    );
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
    // Enter sent and left the line — `Esc` collapses the sheet.
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
}

/// The doc with a user-owned prompt on the producer — the fixture's
/// card stays a card; this is the prompt-peek round's own state.
fn app_with_prompt() -> App {
    let mut app = app_with_doc();
    let mut value = summary_json();
    value["studios"][0]["roles"][0]["attention_user"] = true.into();
    value["studios"][0]["roles"][0]["attention_seq"] = 7.into();
    value["studios"][0]["roles"][0]["attention_detail"] = "may I run the tests?".into();
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app
}

#[tokio::test]
async fn a_prompt_peek_answers_y_and_d() {
    // D5/§4: the peek `p` opens for a prompt carries it — `y`/`d`
    // run the same `approve` argv the row does, then close; the
    // shared peek's keys are unchanged.
    let mut app = app_with_prompt();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);
    alt_y(&mut app);
    app.hollywood.selected = Some(Target::Prompt("acme-shop".into(), "producer".into()));
    key(&mut app, hc_vt::Key::Char('p'));
    assert!(app.peek.is_some(), "the peek opened on the producer");
    // The prompt footer advertises the answers.
    let text = screen_lines(&draw_to(&mut app, 120, 40)).join("\n");
    assert!(
        text.contains("y allow") && text.contains("d deny"),
        "the prompt peek's footer: {text}"
    );
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec![
            "approve",
            "--person",
            "producer",
            "--decision",
            "allow",
            "--studio",
            "acme-shop",
            "--as",
            "user"
        ]]
    );
    assert!(app.peek.is_none(), "the answer closes the peek");
}

#[tokio::test]
async fn a_plain_peek_keeps_its_parity_keys() {
    // A peek that did not come from the Call Sheet knows no prompt:
    // `y` is eaten like every other key, Enter keeps, Esc closes.
    let mut app = app_with_doc();
    app.area = (120, 24);
    app.open_peek("t1".into());
    assert!(app.peek.is_some());
    assert!(app.peek.as_ref().unwrap().prompt.is_none());
    key(&mut app, hc_vt::Key::Char('y'));
    assert!(app.peek.is_some(), "`y` stays eaten in a plain peek");
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.peek.is_none());
}

#[tokio::test]
async fn a_failed_action_shows_a_notice_with_the_stderr_line() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    app.on_hollywood_action(("card apply c3".into(), Err("no card 'c3'".into()), false));
    assert!(app.action_toast.is_some());
    let text = screen_lines(&draw_to(&mut app, 120, 40)).join("\n");
    assert!(text.contains("no card 'c3'"), "{text}");
}

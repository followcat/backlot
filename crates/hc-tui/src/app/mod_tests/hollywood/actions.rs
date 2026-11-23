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
async fn enter_on_a_studio_new_card_runs_card_show_and_opens_the_view() {
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
    key(&mut app, hc_vt::Key::Enter);
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
    key(&mut app, hc_vt::Key::Enter);
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
        card: Some("c3".into()),
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
        card: Some("c3".into()),
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
async fn enter_runs_card_show_on_any_card() {
    // `⏎` reads any card's detail — `s` is stage-start only now
    // (R3-M11), so a card's show rides the row's Enter.
    let mut app = app_with_hollywood();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);
    alt_y(&mut app);
    app.hollywood.selected = Some(Target::Card("c3".into()));
    key(&mut app, hc_vt::Key::Enter);
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
    // R3-2: the send keeps `Compose` for the next tell — the first
    // Esc rests the line, the second collapses the sheet.
    assert!(app.hollywood.editing);
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.hollywood.editing);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
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

/// The doc with one unanswered `user ›` line and a permission prompt
/// on Hollywood's own screen.
fn app_with_hollywood_prompt() -> App {
    let mut app = app_with_hollywood();
    let mut value = summary_json();
    value["hollywood"]["exchange"] =
        serde_json::json!([{ "seq": 8, "who": "user", "text": "recast the producer" }]);
    value["hollywood"]["work"] = "blocked".into();
    value["hollywood"]["attention_user"] = true.into();
    value["hollywood"]["attention_detail"] = "backlot --help".into();
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
async fn y_on_the_hollywood_prompt_row_sends_the_one_time_allow() {
    // The same `approve --decision allow` a role's prompt row sends,
    // on Hollywood's own ledger: the stage turns it into the runner's
    // run-once key, never "don't ask again".
    let mut app = app_with_hollywood_prompt();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);
    alt_y(&mut app);
    app.hollywood.selected = Some(Target::Hollywood);
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec![
            "approve",
            "--person",
            "hollywood",
            "--decision",
            "allow",
            "--as",
            "user"
        ]]
    );
}

#[tokio::test]
async fn a_frame_peek_opens_the_on_set_role_without_a_phantom_prompt() {
    // The footer's `p peek` is live on studio frames too: it opens the
    // role the frame calls on set (the producer here — actor-alice is
    // blocked, not working), and since that seat owns no user prompt
    // the peek carries none — `y` is just eaten, never an answer into
    // thin air.
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    alt_y(&mut app);
    app.hollywood.selected = Some(Target::Studio("acme-shop".into()));
    key(&mut app, hc_vt::Key::Char('p'));
    let Some(peek) = app.peek.as_ref() else {
        panic!("p on a studio frame peeks the on-set role");
    };
    assert!(peek.prompt.is_none(), "no prompt on an unprompted seat");
    key(&mut app, hc_vt::Key::Char('y'));
    assert!(app.peek.is_some(), "`y` stays eaten in a plain peek");
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

#[tokio::test]
async fn enter_on_talk_and_headline_opens_read_only_card_view_and_esc_preserves_selection() {
    let mut app = app_with_hollywood();
    let mut value = summary_json();
    value["hollywood"]["headline_seq"] = serde_json::json!(9);
    // The newest reply is not the headline — otherwise R3-M1's dedup
    // drops the headline row and `Target::Headline` never selects.
    value["hollywood"]["exchange"] = serde_json::json!([
        { "seq": 8, "who": "user", "text": "how are the studios?" },
        { "seq": 10, "who": "hollywood", "text": "beta-tools back on track", "has_body": true }
    ]);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value,
            ..Document::default()
        }
        .into(),
    );
    let (recorded, _) = recorder();
    let sink = recorded.clone();
    app.set_hollywood_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async {
            Ok(
                "acme-shop needs a look\n\n## Report\n- actor-alice blocked on t1\n- card c3 open"
                    .to_string(),
            )
        })
    }));
    app.area = (120, 30);
    alt_y(&mut app);

    // Enter on `Target::Talk(8)` runs `hollywood show 8` instead of focusing `bl-hollywood`.
    let focused_before = app.focused.clone();
    app.hollywood.selected = Some(Target::Talk(8));
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(app.focused, focused_before, "raw terminal not focused");
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec!["hollywood", "show", "8", "--as", "user"]]
    );
    let reply = app.action_rx.recv().await.expect("show reply");
    app.on_action_reply(reply);
    let view = app.hollywood.view.as_ref().expect("read-only CardView");
    assert_eq!(view.card, None);
    assert_eq!(view.title, "hollywood #8");

    // `a` and `x` are inert on a read-only view; `j` scrolls; `Esc` peels back to Call Sheet.
    key(&mut app, hc_vt::Key::Char('a'));
    key(&mut app, hc_vt::Key::Char('x'));
    assert_eq!(recorded.lock().unwrap().len(), 1, "a/x inert on card: None");
    assert!(app.studio_dialog.is_none(), "x opens no cancel dialog");
    key(&mut app, hc_vt::Key::Char('j'));
    assert_eq!(app.hollywood.view.as_ref().unwrap().scroll, 1);

    key(&mut app, hc_vt::Key::Escape);
    assert!(app.hollywood.view.is_none());
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Talk(8)),
        "Esc preserves the selected Talk row"
    );

    // Enter on `Target::Headline` resolves `headline_seq` (9) and opens `hollywood show 9`.
    app.hollywood.selected = Some(Target::Headline);
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().last().unwrap().as_slice(),
        &["hollywood", "show", "9", "--as", "user"]
    );
    let reply = app.action_rx.recv().await.expect("headline show reply");
    app.on_action_reply(reply);
    let view = app.hollywood.view.as_ref().expect("headline CardView");
    assert_eq!(view.card, None);
    assert_eq!(view.title, "hollywood #9");
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.hollywood.view.is_none());
    assert_eq!(app.hollywood.selected, Some(Target::Headline));
}

#[tokio::test]
async fn records_action_opens_read_only_card_view_without_stuffing_card_id() {
    let mut app = app_with_hollywood();
    let (recorded, _) = recorder();
    let sink = recorded.clone();
    app.set_hollywood_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok("runner-a · 3 accepted".to_string()) })
    }));
    app.area = (120, 30);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);

    app.run_action(crate::studio_cmds::records(
        &crate::studio::StudioKey::local("acme-shop"),
    ));
    let reply = app.action_rx.recv().await.expect("records reply");
    app.on_action_reply(reply);

    let view = app.hollywood.view.as_ref().expect("records CardView");
    assert_eq!(view.card, None);
    assert_eq!(view.title, "records acme-shop");
    let text = screen_lines(&draw_to(&mut app, 120, 30)).join("\n");
    assert!(text.contains("records acme-shop"), "{text}");
    assert!(text.contains("↑↓ scroll  esc back"), "{text}");
    assert!(!text.contains("a apply"), "{text}");

    // `a`/`x` do nothing; `Esc` closes the read-only CardView cleanly.
    key(&mut app, hc_vt::Key::Char('a'));
    key(&mut app, hc_vt::Key::Char('x'));
    assert_eq!(recorded.lock().unwrap().len(), 1);
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.hollywood.view.is_none());
}

//! App-level Studio Dock exchange tests: keyboard `Up`/`Down`/`Esc`
//! navigation, `panel_editing()` single-focus invariant, `Enter` and
//! mouse clicks opening local and remote `messages/…` / `reports/…`
//! bodies in the read-only `CardView`, and `m` menu `Read entry`.

use super::*;

type RecordedCall = (Option<String>, Vec<String>);

fn attach_view_runner(
    app: &mut App,
    body: &'static str,
) -> std::sync::Arc<std::sync::Mutex<Vec<RecordedCall>>> {
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::<RecordedCall>::new()));
    let sink = calls.clone();
    app.set_ns_run(std::sync::Arc::new(move |machine, argv, _stdin| {
        sink.lock().unwrap().push((machine, argv));
        Box::pin(async move { Ok(body.to_owned()) })
    }));
    calls
}

async fn drain_actions(app: &mut App) {
    tokio::task::yield_now().await;
    while let Ok(reply) = app.action_rx.try_recv() {
        app.on_action_reply(reply);
    }
}

fn app_with_studio_exchange() -> App {
    let mut app = app_with_panel_doc();
    let mut doc = summary_json();
    doc["studios"][0]["exchange"] = serde_json::json!([
        {
            "seq": 10,
            "who": "user",
            "text": "check checkout flow",
            "file": "messages/10-tell.txt"
        },
        {
            "seq": 11,
            "who": "user",
            "text": "inline legacy note"
        },
        {
            "seq": 12,
            "who": "producer",
            "text": "Checkout verified on staging",
            "file": "reports/12-checkout.md"
        }
    ]);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    app
}

#[tokio::test]
async fn up_down_and_esc_preserve_panel_editing_invariant_across_talk_rows() {
    let mut app = app_with_studio_exchange();
    app.area = (120, 28);
    open_panel(&mut app);
    draw_to(&mut app, 120, 28);

    // `i` focuses `Target::Input` (`panel_editing() == true`).
    key(&mut app, hc_vt::Key::Char('i'));
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    assert!(app.panel_editing());

    // `Esc` exits editing onto `Input` (Resting); `Up` steps onto the newest Talk row (#12).
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.panel_editing());
    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(
        app.panel.selected,
        Some(Target::Talk("acme-shop".into(), 12))
    );
    assert!(!app.panel_editing());

    // `k` and `j` walk cleanly across Talk rows (#11, #10) because we are not editing!
    key(&mut app, hc_vt::Key::Char('k'));
    assert_eq!(
        app.panel.selected,
        Some(Target::Talk("acme-shop".into(), 11))
    );
    assert!(!app.panel_editing());
    assert!(
        app.panel
            .draft(&"acme-shop".into())
            .is_none_or(|draft| draft.as_str().is_empty()),
        "`k` moved, never typed"
    );

    key(&mut app, hc_vt::Key::Char('k'));
    assert_eq!(
        app.panel.selected,
        Some(Target::Talk("acme-shop".into(), 10))
    );
    assert!(!app.panel_editing());

    // `Up` above the oldest Talk row lands on `ClosedFold` (above closed seats).
    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(
        app.panel.selected,
        Some(Target::ClosedFold("acme-shop".into()))
    );
    assert!(!app.panel_editing());

    // `Esc` from `Input` rests the line on the same row — the dock
    // keeps its place and the keys (R3-2); a second `Esc` unfocuses.
    key(&mut app, hc_vt::Key::Char('i'));
    assert!(app.panel_editing());
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.panel.focused);
    assert!(!app.panel_editing());
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.panel.focused);
}

#[tokio::test]
async fn enter_on_talk_opens_read_only_cardview_and_esc_returns_to_dock_row() {
    let mut app = app_with_studio_exchange();
    let calls = attach_view_runner(
        &mut app,
        "# Checkout Report\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\nline 11\nline 12\nline 13\nline 14\nline 15\nline 16\nline 17\nline 18\nline 19\nline 20\n",
    );
    app.area = (120, 20);
    open_panel(&mut app);
    draw_to(&mut app, 120, 20);

    // File-less Talk(#11) is a clean no-op on Enter.
    app.panel.selected = Some(Target::Talk("acme-shop".into(), 11));
    key(&mut app, hc_vt::Key::Enter);
    assert!(calls.lock().unwrap().is_empty());
    assert!(app.hollywood.view.is_none());

    // Talk(#12) runs `backlot report show --file reports/12-checkout.md --studio acme-shop --as user`.
    app.panel.selected = Some(Target::Talk("acme-shop".into(), 12));
    key(&mut app, hc_vt::Key::Enter);
    drain_actions(&mut app).await;
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        &[(
            None,
            vec![
                "report".into(),
                "show".into(),
                "--file".into(),
                "reports/12-checkout.md".into(),
                "--studio".into(),
                "acme-shop".into(),
                "--as".into(),
                "user".into(),
            ]
        )]
    );

    let view = app.hollywood.view.as_ref().expect("CardView opened");
    assert_eq!(view.card, None, "read-only CardView");
    assert_eq!(view.title, "acme-shop #12");

    // CardView renders even while `hollywood.mode == Hidden`, scrolls on `j`, ignores `a`/`x`.
    let screen = screen_lines(&draw_to(&mut app, 120, 20)).join("\n");
    assert!(screen.contains("■ OUTCOME #12 · acme-shop"), "{screen}");
    assert!(
        screen.contains("Checkout Report") && !screen.contains("# Checkout Report"),
        "{screen}"
    );
    assert!(!screen.contains("a apply"), "{screen}");

    key(&mut app, hc_vt::Key::Char('j'));
    assert_eq!(app.hollywood.view.as_ref().unwrap().scroll, 1);
    key(&mut app, hc_vt::Key::Char('a'));
    key(&mut app, hc_vt::Key::Char('x'));
    assert_eq!(
        calls.lock().unwrap().len(),
        1,
        "a/x inert on read-only view"
    );

    // `Esc` closes `CardView` and returns focus straight to `Target::Talk(_, 12)` on the dock.
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.hollywood.view.is_none());
    assert!(app.panel.focused);
    assert!(!app.panel_editing());
    assert_eq!(
        app.panel.selected,
        Some(Target::Talk("acme-shop".into(), 12))
    );
}

#[tokio::test]
async fn remote_studio_talk_click_and_menu_route_report_show_to_remote_machine() {
    let mut app = app_with_two_machines();
    let mut remote_doc = remote_summary_json();
    remote_doc["studios"][0]["exchange"] = serde_json::json!([
        {
            "seq": 4,
            "who": "producer",
            "text": "Remote build passed",
            "file": "messages/4-reply.md"
        }
    ]);
    app.view.documents_mut().insert(
        DocKey {
            machine: Some("host-b".into()),
            name: "backlot".into(),
        },
        Document {
            value: remote_doc,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    let calls = attach_view_runner(&mut app, "Remote producer full message body\n");

    let remote_key = crate::studio::StudioKey {
        machine: Some("host-b".into()),
        id: "beta-tools".into(),
    };
    app.area = (120, 32);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Studio(remote_key.clone()));
    draw_to(&mut app, 120, 32);
    let (rect, target) = app
        .panel_hits
        .rows
        .iter()
        .find(|(_, t)| *t == Target::Talk(remote_key.clone(), 4))
        .cloned()
        .expect("remote Talk(4) row drawn");

    // Left-click selects the remote Talk row (`!panel_editing()`) and opens CardView via host-b.
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 3,
        rect.y,
    ));
    assert_eq!(app.panel.selected, Some(target));
    assert!(!app.panel_editing());
    drain_actions(&mut app).await;

    assert_eq!(
        calls.lock().unwrap().as_slice(),
        &[(
            Some("host-b".into()),
            vec![
                "report".into(),
                "show".into(),
                "--file".into(),
                "messages/4-reply.md".into(),
                "--studio".into(),
                "beta-tools".into(),
                "--as".into(),
                "user".into(),
            ]
        )]
    );
    let view = app.hollywood.view.as_ref().expect("remote CardView opened");
    assert_eq!(view.title, "beta-tools #4 · host-b");
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.hollywood.view.is_none());

    // `m` then Enter on `Read entry #4` dispatches the exact same remote `report show`.
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(app.menu.is_some());
    key(&mut app, hc_vt::Key::Enter);
    drain_actions(&mut app).await;
    assert_eq!(calls.lock().unwrap().len(), 2);
    assert!(app.hollywood.view.is_some());
}

#[tokio::test]
async fn stepping_with_arrows_and_brackets_walks_studio_exchange_and_syncs_dock() {
    let mut app = app_with_panel_doc();
    let mut doc = summary_json();
    doc["studios"][0]["exchange"] = serde_json::json!([
        {
            "seq": 10,
            "who": "user",
            "to": "producer",
            "kind": "tell",
            "text": "check baseline",
            "file": "messages/10-tell.txt"
        },
        {
            "seq": 11,
            "who": "user",
            "to": "producer",
            "kind": "tell",
            "text": "inline blocker check"
        },
        {
            "seq": 12,
            "who": "producer",
            "to": "user",
            "kind": "report",
            "task": "task-1",
            "text": "Checkout verified",
            "file": "reports/12-checkout.md"
        },
        {
            "seq": 13,
            "who": "director",
            "to": "user",
            "kind": "tell",
            "task": "task-2",
            "text": "Dispatch ready",
            "file": "messages/13-dispatch.md"
        }
    ]);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    app.area = (120, 36);

    let calls: std::sync::Arc<std::sync::Mutex<Vec<RecordedCall>>> = Default::default();
    let sink = calls.clone();
    app.set_ns_run(std::sync::Arc::new(move |machine, argv, _stdin| {
        let joined = argv.join(" ");
        sink.lock().unwrap().push((machine, argv));
        let body = if joined.contains("messages/10-tell.txt") {
            "body of #10"
        } else if joined.contains("messages/13-dispatch.md") {
            "body of #13"
        } else {
            "body of #12"
        };
        Box::pin(async move { Ok(body.to_owned()) })
    }));

    open_panel(&mut app);
    app.panel.selected = Some(Target::Talk("acme-shop".into(), 12));
    key(&mut app, hc_vt::Key::Enter);
    drain_actions(&mut app).await;
    let text12 = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(text12.contains("■ REPORT #12 · acme-shop"), "{text12}");
    assert!(text12.contains("◀ 3/4 ▶"), "{text12}");
    assert!(text12.contains("body of #12"), "{text12}");

    // `ArrowRight` starts fetch for #13, syncs `panel.selected` immediately and shows loading tag.
    key(&mut app, hc_vt::Key::ArrowRight);
    assert_eq!(
        app.panel.selected,
        Some(Target::Talk("acme-shop".into(), 13)),
        "dock cursor tracks stepped entry live"
    );
    let loading = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(loading.contains("■ loading #13…"), "{loading}");
    drain_actions(&mut app).await;
    let text13 = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(text13.contains("■ OUTCOME #13 · acme-shop"), "{text13}");
    assert!(text13.contains("◀ 4/4 ▶"), "{text13}");
    assert!(text13.contains("body of #13"), "{text13}");

    // `[` twice steps #13 -> #12 -> inline #11 (no file RPC spawned for #11).
    key(&mut app, hc_vt::Key::Char('['));
    drain_actions(&mut app).await;
    let count_before_inline = calls.lock().unwrap().len();
    key(&mut app, hc_vt::Key::Char('['));
    assert_eq!(
        app.panel.selected,
        Some(Target::Talk("acme-shop".into(), 11))
    );
    assert_eq!(calls.lock().unwrap().len(), count_before_inline);
    let text11 = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(text11.contains("■ TELL #11 · acme-shop"), "{text11}");
    assert!(text11.contains("◀ 2/4 ▶"), "{text11}");
    assert!(text11.contains("inline blocker check"), "{text11}");

    // `]` steps forward again across inline #11 -> #12.
    key(&mut app, hc_vt::Key::Char(']'));
    drain_actions(&mut app).await;
    assert_eq!(
        app.panel.selected,
        Some(Target::Talk("acme-shop".into(), 12))
    );
}

#[tokio::test]
async fn clicking_sibling_talk_in_undimmed_beside_dock_switches_reader_in_place() {
    let mut app = app_with_studio_exchange();
    let calls: std::sync::Arc<std::sync::Mutex<Vec<RecordedCall>>> = Default::default();
    let sink = calls.clone();
    app.set_ns_run(std::sync::Arc::new(move |machine, argv, _stdin| {
        let joined = argv.join(" ");
        sink.lock().unwrap().push((machine, argv));
        let body = if joined.contains("messages/10-tell.txt") {
            "sibling body #10"
        } else {
            "original body #12"
        };
        Box::pin(async move { Ok(body.to_owned()) })
    }));
    app.area = (120, 36);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Talk("acme-shop".into(), 12));
    key(&mut app, hc_vt::Key::Enter);
    drain_actions(&mut app).await;
    draw_to(&mut app, 120, 36);

    let talk10 = app
        .panel_hits
        .rows
        .iter()
        .find(|(_, t)| *t == Target::Talk("acme-shop".into(), 10))
        .map(|(r, _)| *r)
        .expect("Talk(10) hit rect on beside dock");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        talk10.x + 2,
        talk10.y,
    ));
    assert!(
        app.hollywood.view.is_some(),
        "clicking sibling Talk row on undimmed dock must keep Reader open"
    );
    assert_eq!(
        app.panel.selected,
        Some(Target::Talk("acme-shop".into(), 10))
    );
    drain_actions(&mut app).await;
    let text10 = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(text10.contains("■ TELL #10 · acme-shop"), "{text10}");
    assert!(text10.contains("sibling body #10"), "{text10}");

    // Clicking dimmed pane area outside Reader & dock Talk rows still closes Reader.
    app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 1, 1));
    assert!(app.hollywood.view.is_none());
}

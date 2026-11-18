//! Focused-panel keys: movement, flipping, group collapse, the Enter
//! actions (focus, two-step reopen, non-resumable refusal) and the
//! injected `priority` runner with its failure notice.
use super::*;
use crate::panel::Target;
use crate::studio::StudioKey;

/// Bump `updated_ms` in both documents and land the publish — the
/// republish rhythm a live two-machine client sees between presses.
fn republish(app: &mut App) {
    for key in [
        doc_key(),
        DocKey {
            machine: Some("host-b".into()),
            name: "backlot".into(),
        },
    ] {
        if let Some(doc) = app.view.documents.get(&key) {
            let mut value = doc.value.clone();
            if let Some(ms) = value["updated_ms"].as_u64() {
                value["updated_ms"] = (ms + 1).into();
            }
            app.view.documents_mut().insert(
                key,
                Document {
                    value,
                    ..Document::default()
                }
                .into(),
            );
        }
    }
    app.reconcile_published_view();
}

/// A `terminal.run`-recording fake core for the remote runner —
/// the same wire `panel/tests/remote_tests` drives.
fn serve_runs(
    calls: std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
) -> impl FnMut(crate::testcore::FakeCore, bool) -> hc_proto::watch::BoxFuture<()> + Send + Sync + 'static
{
    move |mut core, framed| {
        let calls = calls.clone();
        Box::pin(async move {
            if !framed {
                std::future::pending::<()>().await;
            }
            core.answer_hello(
                true,
                &[
                    "hello",
                    "terminal.run",
                    "terminal.rm",
                    "terminal.kill",
                    "machine.list",
                ],
                &[],
            )
            .await;
            while let Some(frame) = core.try_read_frame().await {
                if frame.channel != 0 || frame.kind != hc_proto::frame::Kind::Data {
                    continue;
                }
                let req: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
                let result = match req["method"].as_str().unwrap_or("") {
                    "terminal.run" => {
                        let params = req["params"].clone();
                        calls.lock().unwrap().push(params.clone());
                        let address = if params["argv"][0] == "sh" {
                            "host-b/r1"
                        } else {
                            "host-b/r2"
                        };
                        let output = if params["argv"][0] == "sh" {
                            "/home/user/.local/bin/backlot\n"
                        } else {
                            ""
                        };
                        let mut result = crate::testcore::minimal_terminal();
                        result["address"] = address.into();
                        result["output"] = output.into();
                        result["wait"] = serde_json::json!({
                            "terminal": address, "ended": true,
                            "reason": "exit", "turn": 1, "exit_code": 0,
                        });
                        result
                    }
                    "terminal.rm" => serde_json::json!({"removed": true}),
                    _ => serde_json::Value::Null,
                };
                core.reply(&req["id"], result).await;
            }
        })
    }
}

#[tokio::test]
async fn focused_panel_keys_move_flip_collapse_and_do_not_leak() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    // Opening selects the first actionable row — the block top, not
    // the title (A2.2).
    assert_eq!(app.panel.selected, Some(Target::Studio("acme-shop".into())));
    // j lands on the first person, k steps back to the block — never
    // on the title or a divider.
    key(&mut app, hc_vt::Key::Char('j'));
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "producer".into()))
    );
    key(&mut app, hc_vt::Key::Char('k'));
    assert_eq!(app.panel.selected, Some(Target::Studio("acme-shop".into())));
    key(&mut app, hc_vt::Key::ArrowUp);
    // PageDown leaps a viewport — the hit map comes from a real draw.
    draw_to(&mut app, 120, 24);
    key(&mut app, hc_vt::Key::PageDown);
    assert!(matches!(
        app.panel.selected,
        Some(Target::Studio(..) | Target::Block(..) | Target::Person(..))
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
async fn space_collapses_and_expands_the_selected_block() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    // Draw once so `panel_hits.body` carries a real height — the
    // stacking decision reads it.
    draw_to(&mut app, 120, 24);
    app.panel.selected = Some(Target::Studio("acme-shop".into()));
    key(&mut app, hc_vt::Key::Char(' '));
    assert!(
        app.panel
            .collapsed_blocks
            .contains(&StudioKey::local("acme-shop"))
    );
    key(&mut app, hc_vt::Key::Char(' '));
    assert!(
        app.panel
            .expanded_blocks
            .contains(&StudioKey::local("acme-shop"))
    );
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
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "producer".into()))
    );
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
async fn esc_disarms_the_closed_hint() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-dave".into()));
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.panel.closed_hint.is_some());
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.panel.closed_hint.is_none(), "Esc drops the hint");
    // Esc also left focus; the chord re-focuses and dave's next Enter
    // shows the hint again.
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('y'));
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        app.panel.closed_hint,
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
    app.set_panel_config(config, notice, &path);
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
async fn activating_another_row_disarms_the_closed_hint() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-dave".into()));
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.panel.closed_hint.is_some());
    // Enter on a different person drops dave's hint.
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.panel.closed_hint.is_none());
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
async fn a_closed_person_reopens_on_o() {
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
        app.panel.closed_hint,
        Some(("acme-shop".into(), "actor-dave".into()))
    );
    assert!(recorded.lock().unwrap().is_empty(), "Enter only hints");
    key(&mut app, hc_vt::Key::Char('o'));
    assert!(app.panel.closed_hint.is_none());
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
    assert!(app.panel.closed_hint.is_none());
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

#[tokio::test]
async fn keys_reach_a_remote_actor_across_publishes() {
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
    let (accepted, rx) = mpsc::channel(4);
    let (rpc, _sub) = crate::testcore::openers(accepted);
    let fake = crate::testcore::spawn_fake(rx, serve_runs(calls.clone()));
    let mut app = app_with_two_machines();
    app.set_remote_runner(crate::panel::RemoteRunner::new(rpc));
    app.area = (160, 47);
    open_panel(&mut app);
    draw_to(&mut app, 160, 47);
    // `j` once per target past acme-shop's block to `actor-a` on
    // `host-b` — a document republish lands between every press, so
    // the selection must survive rebuilt row lists to get there.
    let actor = Target::Person(
        StudioKey {
            machine: Some("host-b".into()),
            id: "beta-tools".into(),
        },
        "actor-a".into(),
    );
    for _ in 0..30 {
        if app.panel.selected == Some(actor.clone()) {
            break;
        }
        republish(&mut app);
        key(&mut app, hc_vt::Key::Char('j'));
    }
    assert_eq!(app.panel.selected, Some(actor.clone()));
    // `-` lowers the actor's high task to normal — on host-b.
    key(&mut app, hc_vt::Key::Char('-'));
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if calls.lock().unwrap().len() >= 2 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the remote action reached the core");
    let runs = calls.lock().unwrap();
    let action = &runs[1];
    assert_eq!(action["on"], "host-b");
    assert_eq!(
        action["argv"],
        serde_json::json!([
            "/home/user/.local/bin/backlot",
            "priority",
            "--task",
            "t1",
            "--level",
            "normal",
            "--studio",
            "beta-tools",
            "--as",
            "user",
        ])
    );
    drop(runs);
    // Enter focuses the actor's machine-qualified terminal.
    republish(&mut app);
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(app.focused.as_deref(), Some("host-b/t10"));
    fake.abort();
}

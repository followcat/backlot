use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::*;
use crate::hollywood::InlineTellState;
use crate::studio::StudioKey;

type RecordedCalls = Vec<(Option<String>, Vec<String>)>;

fn key_mods(app: &mut App, k: hc_vt::Key, mods: hc_vt::Mods) {
    app.on_input(
        UiEvent::Key(term_key(k, mods, hc_vt::Action::Press)),
        app.area.0,
    );
}

fn compose_fixture_app() -> App {
    let mut app = app_with_panel_doc();
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = crate::app::pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    app.set_hollywood(
        Ok(Some(backlot_stage::config::HollywoodCfg {
            runner: "agy".into(),
            model: "pro".into(),
            effort: Some("high".into()),
            key: None,
            default_cast: None,
        })),
        std::path::Path::new("config.toml"),
    );
    app.view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        Document {
            value: serde_json::json!({
                "schema": "backlot/1",
                "updated_ms": 1_700_000_060_000u64,
                "studios": [{
                    "id": "acme-shop",
                    "title": "Acme shop",
                    "state": "live",
                    "requirement_version": 2,
                    "mode": "standard",
                    "open_reviews": 1,
                    "waiting": 0,
                    "next": "review t2 — director",
                    "roles": [
                        { "role": "producer", "name": "producer", "terminal": "t1", "runner": "agy", "model": "pro", "state": "live", "work": "ready" },
                        { "role": "director", "name": "director", "terminal": "t2", "runner": "agy", "model": "pro", "state": "live", "work": "working" },
                        { "role": "actor-eve", "name": "eve", "terminal": null, "runner": "agy", "model": "pro", "state": "closed", "work": null, "resumable": false }
                    ],
                    "tasks": [{
                        "id": "t1",
                        "title": "First",
                        "status": "running",
                        "priority": "normal",
                        "requirement_version": 2,
                        "rework_rounds": 0,
                        "rework_cap": 3,
                        "actor_role": "director",
                        "actor_name": "director"
                    }],
                    "exchange": [
                        { "seq": 10, "who": "producer", "to": "user", "kind": "report", "task": "t1", "ts_ms": 1_700_000_000_000u64, "file": "reports/10.md", "text": "producer ready" },
                        { "seq": 11, "who": "director", "to": "user", "kind": "report", "task": "t1", "ts_ms": 1_700_000_030_000u64, "file": "reports/11.md", "text": "director waiting review" },
                        { "seq": 12, "who": "actor-eve", "to": "user", "kind": "outcome", "task": "t1", "ts_ms": 1_700_000_045_000u64, "text": "eve done" }
                    ]
                }],
                "hollywood": {
                    "state": "live",
                    "runner": "agy",
                    "model": "pro",
                    "effort": "high",
                    "headline": "hollywood coordinating acme-shop",
                    "exchange": [
                        { "seq": 5, "who": "hollywood", "to": "user", "kind": "report", "ts_ms": 1_700_000_050_000u64, "text": "hollywood status report" }
                    ],
                    "cards": [
                        { "id": "c3", "studio": "acme-shop", "action": "pause t1", "why": "loop check", "ts_ms": 1_700_000_055_000u64 }
                    ]
                }
            }),
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.hollywood.mode = crate::hollywood::LayerMode::Collapsed;
    app
}

fn mock_runner(app: &mut App) -> Arc<Mutex<RecordedCalls>> {
    let calls = Arc::new(Mutex::new(RecordedCalls::new()));
    let sink = calls.clone();
    let status = serde_json::json!({
        "schema": "studio/1",
        "id": "acme-shop",
        "requirement_version": 2,
        "last_seq": 12,
        "tasks": {
            "t1": {
                "id": "t1",
                "title": "First",
                "status": "delivered",
                "task_kind": "task",
                "requirement_version": 2,
                "priority": "normal",
                "acceptance": "passes checks",
                "rework_rounds": 0,
                "dependencies": [],
                "created_seq": 1,
                "created_ts_ms": 1,
                "last_ts_ms": 1,
                "deliveries": [{
                    "n": 1,
                    "actor": { "runner": "agy", "model": "pro", "permission_profile": "standard" },
                    "commits": "a1..b2",
                    "requirement_version": 2,
                    "report": "reports/11.md",
                    "checks": [],
                    "seq": 11,
                    "ts_ms": 2
                }],
                "checked": [],
                "reviews": []
            }
        }
    })
    .to_string();
    app.set_ns_run(Arc::new(move |machine, argv, _stdin| {
        sink.lock().unwrap().push((machine, argv.clone()));
        let cmd = argv.first().map(String::as_str).unwrap_or("");
        let out = match cmd {
            "report" => (1..=18)
                .map(|i| format!("▪ Verified detail line {i} in crates/hc-tui/src/app/mod.rs"))
                .collect::<Vec<_>>()
                .join("\n"),
            "hollywood" if argv.get(1).map(String::as_str) == Some("show") => {
                "Hollywood detail body line".to_string()
            }
            "status" => status.clone(),
            "records" => "[]".to_string(),
            _ => "sent ok".to_string(),
        };
        Box::pin(async move { Ok(out) })
    }));
    calls
}

async fn drain_actions(app: &mut App) {
    tokio::task::yield_now().await;
    while let Ok(res) = app.action_rx.try_recv() {
        app.on_action_reply(res);
    }
    while let Ok(res) = app.sheet_rx.try_recv() {
        app.on_sheet_reply(res);
    }
}

fn type_str(app: &mut App, s: &str) {
    for ch in s.chars() {
        key(app, hc_vt::Key::Char(ch));
    }
}

#[tokio::test]
async fn reader_t_shares_draft_with_dock_seeds_non_producer_and_esc_twice_keeps_draft() {
    let mut app = compose_fixture_app();
    let _calls = mock_runner(&mut app);
    app.area = (120, 36);
    open_panel(&mut app);

    // Non-producer live role (`director`, seq 11) with empty draft seeds `/tell director `.
    app.panel.selected = Some(Target::Talk("acme-shop".into(), 11));
    key(&mut app, hc_vt::Key::Enter);
    drain_actions(&mut app).await;
    assert!(app.hollywood.view.is_some());

    key(&mut app, hc_vt::Key::Char('t'));
    assert_eq!(
        app.hollywood.view.as_ref().unwrap().tell,
        InlineTellState::Composing
    );
    let studio_key = StudioKey::local("acme-shop");
    assert_eq!(
        app.panel.draft(&studio_key).map(|d| d.as_str()),
        Some("/tell director ")
    );
    type_str(&mut app, "check t1");
    assert_eq!(
        app.panel.draft(&studio_key).map(|d| d.as_str()),
        Some("/tell director check t1")
    );

    // Draw at 120x36: hardware cursor lands inside compose row & undimmed dock mirrors draft.
    let screen = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(screen.contains("› tell director:"), "{screen}");
    assert!(screen.contains("/tell director check t1"), "{screen}");
    let hits = app.hollywood_hits.view.as_ref().unwrap();
    let compose_rect = hits.compose_rect.expect("compose hit rect");
    let cursor = app.want_cursor.pos.expect("compose hardware cursor");
    assert_eq!(cursor.1, compose_rect.y);

    // First Esc leaves Composing -> Reading while keeping Reader open and draft intact.
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.hollywood.view.is_some());
    assert_eq!(
        app.hollywood.view.as_ref().unwrap().tell,
        InlineTellState::Reading
    );
    assert_eq!(
        app.panel.draft(&studio_key).map(|d| d.as_str()),
        Some("/tell director check t1")
    );

    // Second Esc closes Reader, keeping draft intact on beside Lot Dock; clicking `[t]` re-opens Composing.
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.hollywood.view.is_none());

    app.panel.selected = Some(Target::Talk("acme-shop".into(), 10));
    key(&mut app, hc_vt::Key::Enter);
    drain_actions(&mut app).await;
    draw_to(&mut app, 120, 36);
    let t_chip = app
        .hollywood_hits
        .view
        .as_ref()
        .unwrap()
        .chips
        .iter()
        .find(|(_, ch)| *ch == 't')
        .map(|(r, _)| *r)
        .expect("[t] chip hit rect");
    app.on_mouse(MouseInput {
        kind: MouseEventKind::Down(MouseButton::Left),
        col: t_chip.x,
        row: t_chip.y,
        mods: hc_vt::Mods::NONE,
        sgr_position: None,
    });
    assert_eq!(
        app.hollywood.view.as_ref().unwrap().tell,
        InlineTellState::Composing
    );
    assert_eq!(
        app.panel.draft(&studio_key).map(|d| d.as_str()),
        Some("/tell director check t1")
    );
}

#[tokio::test]
async fn reader_compose_submit_lifecycle_sent_timer_failure_retry_and_mistype_guard() {
    let mut app = compose_fixture_app();
    let calls = mock_runner(&mut app);
    app.area = (120, 36);
    open_panel(&mut app);

    app.panel.selected = Some(Target::Talk("acme-shop".into(), 10));
    key(&mut app, hc_vt::Key::Enter);
    drain_actions(&mut app).await;

    // Open with `i`, Enter on empty returns to Reading.
    key(&mut app, hc_vt::Key::Char('i'));
    assert_eq!(
        app.hollywood.view.as_ref().unwrap().tell,
        InlineTellState::Composing
    );
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        app.hollywood.view.as_ref().unwrap().tell,
        InlineTellState::Reading
    );

    // Mistyped `/verb` stays Composing and shows inline notice without sending.
    key(&mut app, hc_vt::Key::Char('t'));
    type_str(&mut app, "i/pause t1");
    let calls_before = calls.lock().unwrap().len();
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        app.hollywood.view.as_ref().unwrap().tell,
        InlineTellState::Composing
    );
    assert!(app.notice.is_some());
    assert_eq!(calls.lock().unwrap().len(), calls_before);

    // Tab `/verb` completion in Reader composer (`panel_input_complete_for`).
    let studio_key = StudioKey::local("acme-shop");
    app.panel.draft_mut(&studio_key).clear();
    type_str(&mut app, "/res");
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(
        app.panel.draft(&studio_key).map(|d| d.as_str()),
        Some("/resume ")
    );

    // Replace with plain tell and Enter -> `Sending` -> `Ok` -> `Sent` -> auto-collapse at 1.5s.
    app.panel.draft_mut(&studio_key).clear();
    type_str(&mut app, "please run checks");
    key(&mut app, hc_vt::Key::Enter);
    assert!(matches!(
        app.hollywood.view.as_ref().unwrap().tell,
        InlineTellState::Sending { .. }
    ));
    let sending_screen = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(
        sending_screen.contains("■ sending to producer…"),
        "{sending_screen}"
    );
    // Second `t` press while Sending is locked.
    key(&mut app, hc_vt::Key::Char('t'));
    assert!(matches!(
        app.hollywood.view.as_ref().unwrap().tell,
        InlineTellState::Sending { .. }
    ));

    let res = app.action_rx.recv().await.expect("tell reply");
    app.on_action_reply(res);
    assert!(matches!(
        app.hollywood.view.as_ref().unwrap().tell,
        InlineTellState::Sent { .. }
    ));
    assert!(app.wake_deadline().is_some());
    let sent_screen = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(sent_screen.contains("■ sent to producer"), "{sent_screen}");

    // Scrolling or 1.5s timer collapses `Sent` back to `Reading`.
    assert!(app.expire_reader_sent(Instant::now() + Duration::from_secs(2)));
    assert_eq!(
        app.hollywood.view.as_ref().unwrap().tell,
        InlineTellState::Reading
    );

    // Error reply restores draft into shared buffer and offers Enter retry -> Composing.
    key(&mut app, hc_vt::Key::Char('t'));
    type_str(&mut app, "retry me");
    key(&mut app, hc_vt::Key::Enter);
    let err_res = app.action_rx.recv().await.expect("second tell");
    app.on_action_reply((err_res.0, Err("stage offline\ndetails".into())));
    assert!(matches!(
        app.hollywood.view.as_ref().unwrap().tell,
        InlineTellState::Failed { .. }
    ));
    assert_eq!(
        app.panel.draft(&studio_key).map(|d| d.as_str()),
        Some("retry me")
    );
    let failed_screen = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(
        failed_screen.contains("■ send failed: stage offline"),
        "{failed_screen}"
    );
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        app.hollywood.view.as_ref().unwrap().tell,
        InlineTellState::Composing
    );
    assert_eq!(
        app.panel.draft(&studio_key).map(|d| d.as_str()),
        Some("retry me")
    );
}

#[tokio::test]
async fn reader_and_dock_share_up_down_history_for_studio_hollywood_and_sheet() {
    let mut app = compose_fixture_app();
    let calls = mock_runner(&mut app);
    app.area = (120, 36);
    open_panel(&mut app);
    let studio_key = StudioKey::local("acme-shop");

    // Submit once from Reader -> recalls via Up in Dock `›` row!
    app.panel.selected = Some(Target::Talk("acme-shop".into(), 10));
    key(&mut app, hc_vt::Key::Enter);
    drain_actions(&mut app).await;
    key(&mut app, hc_vt::Key::Char('t'));
    type_str(&mut app, "first from reader");
    key(&mut app, hc_vt::Key::Enter);
    drain_actions(&mut app).await;
    // Scroll dismisses Sent cleanly back to Reading.
    key(&mut app, hc_vt::Key::Char('j'));
    assert_eq!(
        app.hollywood.view.as_ref().unwrap().tell,
        InlineTellState::Reading
    );
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.hollywood.view.is_none());

    // Enter Dock Compose with unsent draft and press Up -> recalls Reader submit, Down restores stash!
    app.panel.selected = Some(Target::Input(studio_key.clone()));
    key(&mut app, hc_vt::Key::Char('i'));
    type_str(&mut app, "unsent dock");
    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(
        app.panel.draft(&studio_key).map(|d| d.as_str()),
        Some("first from reader")
    );
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(
        app.panel.draft(&studio_key).map(|d| d.as_str()),
        Some("unsent dock")
    );
    key(&mut app, hc_vt::Key::Escape);
    app.panel.draft_mut(&studio_key).clear();

    // Hollywood entry in Reader uses `hollywood.input` + `hollywood.history`.
    app.hollywood.editing = false;
    key_mods(&mut app, hc_vt::Key::Char('y'), hc_vt::Mods::ALT);
    app.hollywood_select(crate::hollywood::Target::Talk(5));
    key(&mut app, hc_vt::Key::Enter);
    drain_actions(&mut app).await;
    key(&mut app, hc_vt::Key::Char('t'));
    type_str(&mut app, "ping hollywood");
    assert_eq!(app.hollywood.input.as_str(), "ping hollywood");
    key(&mut app, hc_vt::Key::Enter);
    drain_actions(&mut app).await;
    assert!(app.hollywood.input.as_str().is_empty());

    // Escape Reader -> back on Call Sheet `i`, Up recalls Reader Hollywood send.
    key(&mut app, hc_vt::Key::Escape);
    key(&mut app, hc_vt::Key::Escape);
    key(&mut app, hc_vt::Key::Char('i'));
    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(app.hollywood.input.as_str(), "ping hollywood");
    key(&mut app, hc_vt::Key::Escape);
    key(&mut app, hc_vt::Key::Escape);

    // Sheet delivery artifact Reader also supports inline `t` compose sharing studio draft.
    app.sheet_open(studio_key.clone(), Some("t1".into()));
    drain_actions(&mut app).await;
    app.sheet.as_mut().unwrap().tab = crate::sheet::SheetTab::Deliveries;
    key(&mut app, hc_vt::Key::Enter);
    drain_actions(&mut app).await;
    assert!(app.sheet.as_ref().unwrap().view.is_some());
    key(&mut app, hc_vt::Key::Char('t'));
    type_str(&mut app, "check delivery file");
    assert_eq!(
        app.panel.draft(&studio_key).map(|d| d.as_str()),
        Some("check delivery file")
    );
    let before = calls.lock().unwrap().len();
    key(&mut app, hc_vt::Key::Enter);
    drain_actions(&mut app).await;
    assert!(calls.lock().unwrap()[before..].iter().any(|(_, argv)| {
        argv.join(" ")
            .contains("tell producer --text check delivery file")
    }));
}

#[tokio::test]
async fn slice5_visual_shots() {
    let Ok(dir) = std::env::var("LOT_READER_SHOTS") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let mut app = compose_fixture_app();
    let _calls = mock_runner(&mut app);
    app.area = (120, 36);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Talk("acme-shop".into(), 11));
    key(&mut app, hc_vt::Key::Enter);
    drain_actions(&mut app).await;
    key(&mut app, hc_vt::Key::Char('t'));
    type_str(&mut app, "请先合入 T1 再跑回归");

    for (name, w, h) in [
        ("s5-80x24.txt", 80u16, 24u16),
        ("s5-120x36.txt", 120, 36),
        ("s5-200x44.txt", 200, 44),
    ] {
        app.area = (w, h);
        let lines = screen_lines(&draw_to(&mut app, w, h));
        if w == 80 && h == 24 {
            let hits = app.hollywood_hits.view.as_ref().unwrap();
            assert!(
                hits.area.bottom() <= h.saturating_sub(1),
                "80x24 compose overflowed: {:?}",
                hits.area
            );
        }
        std::fs::write(format!("{dir}/{name}"), lines.join("\n")).unwrap();
    }
}

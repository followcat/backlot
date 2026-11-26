//! Tests for unified `HwSwitch` picker, `w` entry points, decoupled
//! `bl-hollywood` terminal resolution, and Call Sheet / Lot Pill
//! switch state rendering (§2.2, §2.4.2).

use super::*;
use crate::hollywood::Target;

fn switch_runners_json() -> String {
    serde_json::json!([
        { "runner": "claude", "status": "found", "version": "2",
          "models": ["claude-sonnet-5-5", "claude-opus-5-5"], "efforts": ["low", "high"] },
        { "runner": "codex", "status": "found", "version": "1",
          "models": ["gpt-6-sol"], "efforts": ["high"] }
    ])
    .to_string()
}

fn install_switch_runner(app: &mut App) -> std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>> {
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_ns_run(std::sync::Arc::new(move |_machine, argv, _stdin| {
        sink.lock().unwrap().push(argv.clone());
        let out = if argv.first().map(String::as_str) == Some("runners") {
            Ok(switch_runners_json())
        } else {
            Ok("[hollywood] ok".to_string())
        };
        Box::pin(async move { out })
    }));
    recorded
}

async fn drain(app: &mut App) {
    for _ in 0..20 {
        tokio::task::yield_now().await;
        while let Ok(reply) = app.ns_rx.try_recv() {
            app.on_ns_reply(reply);
        }
    }
}

fn set_doc(app: &mut App, doc: serde_json::Value) {
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
}

#[tokio::test]
async fn w_opens_unified_picker_refuses_unchanged_and_highlights_alt_on_quota() {
    let mut app = app_with_hollywood();
    app.set_hollywood(
        Ok(Some(HollywoodCfg {
            runner: "claude".into(),
            model: "claude-sonnet-5-5".into(),
            effort: Some("high".into()),
            key: Some("alt+y".into()),
            default_cast: None,
        })),
        std::path::Path::new("config.toml"),
    );
    let recorded = install_switch_runner(&mut app);
    app.area = (120, 30);
    alt_y(&mut app);
    key(&mut app, hc_vt::Key::Escape);

    // `w` anywhere in Call Sheet Resting opens HwSwitch prefilled with current identity.
    key(&mut app, hc_vt::Key::Char('w'));
    assert!(app.hw_switch.is_some());
    drain(&mut app).await;
    let screen = screen_lines(&draw_to(&mut app, 120, 30)).join("\n");
    assert!(
        screen.contains("Current: claude / claude-sonnet-5-5 / high"),
        "{screen}"
    );
    assert!(
        screen.contains("make-before-break: old agent stays live"),
        "{screen}"
    );

    // Submitting unchanged identity keeps modal open and shows error without spawning CLI.
    for _ in 0..3 {
        key(&mut app, hc_vt::Key::ArrowDown);
    }
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.hw_switch.is_some(), "unchanged identity stays open");
    assert_eq!(recorded.lock().unwrap().len(), 1, "only runners probe ran");
    key(&mut app, hc_vt::Key::Escape);

    // Out-of-quota state: Wait::Hollywood offers `[switch w]` primary chip and pre-selects codex.
    let mut doc = summary_json();
    doc["hollywood"]["runner"] = serde_json::json!("claude");
    doc["hollywood"]["model"] = serde_json::json!("claude-sonnet-5-5");
    doc["hollywood"]["effort"] = serde_json::json!("high");
    doc["hollywood"]["attention"] = serde_json::json!("out of quota");
    doc["hollywood"]["attention_detail"] = serde_json::json!("weekly limit reached");
    doc["hollywood"]["attention_user"] = serde_json::json!(true);
    doc["hollywood"]["can_answer_prompt"] = serde_json::json!(false);
    doc["hollywood"]["queue"] = serde_json::json!({
        "count": 1,
        "reason": "out_of_quota",
        "detail": "weekly limit reached"
    });
    set_doc(&mut app, doc);
    app.hollywood_select(Target::Hollywood);
    key(&mut app, hc_vt::Key::Enter); // activates primary `switch w` chip!
    assert!(app.hw_switch.is_some());
    drain(&mut app).await;
    let d = app.hw_switch.as_ref().unwrap();
    assert_eq!(d.runner_row().unwrap().runner, "codex");
}

#[tokio::test]
async fn hollywood_terminal_and_candidate_peek_decoupled_from_hardcoded_name() {
    let mut app = app_with_hollywood();
    let mut old_hw = crate::sidebar::fixture::term("t11");
    old_hw.name = Some("bl-hollywood-g2".into());
    old_hw.state = "live".into();
    old_hw
        .labels
        .insert("backlot.role".into(), "hollywood".into());
    let mut cand_hw = crate::sidebar::fixture::term("t12");
    cand_hw.name = Some("bl-hollywood-next".into());
    cand_hw.state = "live".into();
    cand_hw
        .labels
        .insert("backlot.role".into(), "hollywood-handoff".into());
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    app.view
        .inventory_mut()
        .replace(3, vec![stage, old_hw, cand_hw]);

    let mut doc = summary_json();
    doc["hollywood"]["terminal"] = serde_json::json!("t11");
    doc["hollywood"]["handoff"] = serde_json::json!({
        "id": "h-1",
        "phase": "verifying",
        "target_runner": "codex",
        "target_model": "gpt-6-sol",
        "target_effort": "high",
        "candidate_terminal": "t12",
        "started_ms": crate::update_state::now_ms().saturating_sub(14_000),
        "timeout_ms": 90_000
    });
    set_doc(&mut app, doc);

    assert_eq!(app.hollywood_terminal().as_deref(), Some("t11"));
    assert_eq!(app.hollywood_live_terminal().as_deref(), Some("t11"));
    assert_eq!(app.candidate_hollywood_terminal().as_deref(), Some("t12"));

    app.area = (120, 30);
    alt_y(&mut app);
    app.hollywood_select(Target::Hollywood);
    // `peek next p` peeks candidate terminal `t12`.
    key(&mut app, hc_vt::Key::Char('p'));
    let peek = app.peek.as_ref().expect("peek opened");
    assert_eq!(peek.terminal, "t12");
    key(&mut app, hc_vt::Key::Escape);

    // `Enter` on Title focuses active terminal `t11` (not hardcoded bl-hollywood).
    app.hollywood_select(Target::Title);
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(app.focused.as_deref(), Some("t11"));
}

#[tokio::test]
async fn switch_lifecycle_states_render_call_sheet_pill_and_toasts() {
    let mut app = app_with_hollywood();
    let recorded = install_switch_runner(&mut app);
    app.shell_state.sidebar_collapsed = true;
    app.panel.mode = crate::panel::PanelMode::Closed;
    app.area = (100, 18);
    let now = crate::update_state::now_ms();
    let dump = |name: &str, text: &str| {
        if std::env::var_os("HC_DUMP_SHEET").is_some() {
            eprintln!("=== DUMP {name} (100x18) ===\n{text}\n=== END {name} ===");
        }
    };

    // 0. Idle state (healthy runner, Call Sheet open at 100x18).
    alt_y(&mut app);
    app.hollywood_select(Target::Hollywood);
    let sheet_idle = screen_lines(&draw_to(&mut app, 100, 18)).join("\n");
    dump("idle", &sheet_idle);
    app.hollywood_collapse();

    let base_hw = |doc: &mut serde_json::Value| {
        doc["hollywood"]["runner"] = serde_json::json!("claude");
        doc["hollywood"]["model"] = serde_json::json!("sonnet-5-5");
        doc["hollywood"]["attention"] = serde_json::json!("out of quota");
        doc["hollywood"]["attention_detail"] = serde_json::json!("weekly limit reached");
        doc["hollywood"]["attention_user"] = serde_json::json!(true);
        doc["hollywood"]["can_answer_prompt"] = serde_json::json!(false);
        doc["hollywood"]["pending"] = serde_json::json!(true);
        doc["hollywood"]["queue"] = serde_json::json!({
            "count": 1,
            "reason": "out_of_quota",
            "detail": "weekly limit reached"
        });
        doc["hollywood"]["exchange"] = serde_json::json!([
            { "seq": 10, "who": "user", "text": "Pause beta-tools and summarize acme-shop." }
        ]);
    };

    // 1. Quota-out pre-switch: Pill & Call Sheet.
    let mut doc = summary_json();
    base_hw(&mut doc);
    set_doc(&mut app, doc.clone());
    let pill_quota = screen_lines(&draw_to(&mut app, 100, 18))[0].clone();
    assert!(pill_quota.contains("(quota)"), "quota pill: {pill_quota}");
    alt_y(&mut app);
    app.hollywood_select(Target::Hollywood);
    let sheet_quota = screen_lines(&draw_to(&mut app, 100, 18)).join("\n");
    dump("quota-out", &sheet_quota);
    assert!(sheet_quota.contains("out of quota"), "{sheet_quota}");
    assert!(sheet_quota.contains("switch"), "{sheet_quota}");

    // 2. `preparing` -> `starting` -> `verifying`: Pill shows `switching…`, Sheet shows details & cancel x.
    for (phase, needle) in [
        ("preparing", "preparing brief…"),
        ("starting", "starting…"),
        ("verifying", "verifying (14s)"),
    ] {
        doc["hollywood"]["handoff"] = serde_json::json!({
            "id": "h-7",
            "phase": phase,
            "target_runner": "codex",
            "target_model": "gpt-6-sol",
            "target_effort": "high",
            "candidate_terminal": "t10",
            "started_ms": now.saturating_sub(14_000),
            "timeout_ms": 90_000
        });
        doc["hollywood"]["queue"] = serde_json::json!({
            "count": 1,
            "reason": "handoff",
            "detail": "codex/gpt-6-sol/high"
        });
        set_doc(&mut app, doc.clone());
        app.hollywood_collapse();
        let pill_sw = screen_lines(&draw_to(&mut app, 100, 18))[0].clone();
        assert!(pill_sw.contains("switching…"), "{phase} pill: {pill_sw}");

        app.hollywood_open();
        app.hollywood_select(Target::Hollywood);
        let sheet_sw = screen_lines(&draw_to(&mut app, 100, 18)).join("\n");
        if matches!(phase, "preparing" | "verifying") {
            dump(phase, &sheet_sw);
        }
        assert!(sheet_sw.contains(needle), "{phase}: {sheet_sw}");
        assert!(
            sheet_sw.contains("old session kept live"),
            "{phase}: {sheet_sw}"
        );
        assert!(
            sheet_sw.contains("draft kept across switch"),
            "{phase}: {sheet_sw}"
        );
        if phase == "verifying" {
            assert!(sheet_sw.contains("peek next"), "{sheet_sw}");
            assert!(sheet_sw.contains("cancel"), "{sheet_sw}");
            assert!(
                sheet_sw.contains("waiting for handoff to codex/gpt-6-sol/high"),
                "{phase}: {sheet_sw}"
            );
        }
    }

    // Pressing `x` on Target::Hollywood during verifying sends `--cancel`.
    key(&mut app, hc_vt::Key::Char('x'));
    assert_eq!(
        recorded.lock().unwrap().last().unwrap(),
        &vec!["hollywood", "switch", "--cancel", "--as", "user"]
    );

    // 3. `needs_approval`: Pill shows `!switch prompt`, Sheet shows `[allow y]` and sends approve allow.
    doc["hollywood"]["handoff"]["phase"] = serde_json::json!("needs_approval");
    doc["hollywood"]["attention"] = serde_json::json!("permission prompt");
    doc["hollywood"]["attention_detail"] = serde_json::json!("cargo test");
    doc["hollywood"]["attention_user"] = serde_json::json!(true);
    set_doc(&mut app, doc.clone());
    app.hollywood_collapse();
    let pill_ap = screen_lines(&draw_to(&mut app, 100, 18))[0].clone();
    assert!(pill_ap.contains("!switch prompt"), "{pill_ap}");
    app.hollywood_open();
    app.hollywood_select(Target::Hollywood);
    let sheet_ap = screen_lines(&draw_to(&mut app, 100, 18)).join("\n");
    dump("needs_approval", &sheet_ap);
    assert!(sheet_ap.contains("needs approval"), "{sheet_ap}");
    assert!(sheet_ap.contains("cargo test"), "{sheet_ap}");
    assert!(sheet_ap.contains("allow"), "{sheet_ap}");
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(
        recorded.lock().unwrap().last().unwrap(),
        &vec![
            "approve",
            "--person",
            "hollywood",
            "--decision",
            "allow",
            "--as",
            "user"
        ]
    );

    // 4. `rolled_back`: inline failure banner + error toast + `[switch w]`.
    doc["hollywood"]["handoff"]["phase"] = serde_json::json!("rolled_back");
    doc["hollywood"]["handoff"]["error"] =
        serde_json::json!("candidate codex/gpt-6-sol hit quota limit");
    set_doc(&mut app, doc.clone());
    assert!(
        app.action_toast
            .as_ref()
            .is_some_and(|t| t.fail && t.body.contains("hit quota limit"))
    );
    let sheet_rb = screen_lines(&draw_to(&mut app, 100, 18)).join("\n");
    dump("rolled_back", &sheet_rb);
    assert!(sheet_rb.contains("hit quota limit"), "{sheet_rb}");
    assert!(sheet_rb.contains("switch"), "{sheet_rb}");

    // 5. `committed`: transitions from active id -> updates cfg and shows success toast.
    doc["hollywood"]["handoff"] = serde_json::json!({
        "id": "h-8",
        "phase": "verifying",
        "target_runner": "codex",
        "target_model": "gpt-6-sol",
        "target_effort": "high",
        "started_ms": now,
        "timeout_ms": 90_000
    });
    set_doc(&mut app, doc.clone());
    doc["hollywood"]["runner"] = serde_json::json!("codex");
    doc["hollywood"]["model"] = serde_json::json!("gpt-6-sol");
    doc["hollywood"]["effort"] = serde_json::json!("high");
    doc["hollywood"]["attention"] = serde_json::Value::Null;
    doc["hollywood"]["attention_detail"] = serde_json::Value::Null;
    doc["hollywood"]["attention_user"] = serde_json::json!(false);
    doc["hollywood"]["pending"] = serde_json::json!(false);
    doc["hollywood"]["queue"] = serde_json::Value::Null;
    doc["hollywood"]["handoff"]["phase"] = serde_json::json!("committed");
    set_doc(&mut app, doc);
    assert_eq!(app.hollywood_cfg.as_ref().unwrap().runner, "codex");
    assert!(app.action_toast.as_ref().is_some_and(|t| {
        !t.fail
            && t.title
                .contains("switched hollywood to codex/gpt-6-sol/high")
    }));
}

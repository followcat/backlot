//! PR 7 App-level end-to-end fail-first & regression tests:
//! Pill `?N` + Call Sheet `DECISION` card + Lot Dock expanded & 16-col strip at 200x44 & 100x40,
//! interactive `a` confirm/cancel (`sign off` vs `skip`, frozen file), `r` reply/`design open`,
//! `o` `design show` Reader, and zero-design invariance.
//! Neutral fixtures only (`acme-shop`, `beta-api`, `producer`).

use std::sync::{Arc, Mutex};

use hc_proto::watch::Document;

use super::*;
use crate::hollywood::LayerMode;
use crate::panel::PanelMode;
use crate::studio::StudioKey;

type Recorded = Arc<Mutex<Vec<(Option<String>, Vec<String>)>>>;

fn ns_recorder(show_body: &'static str) -> (Recorded, crate::app::new_studio::NsRun) {
    let calls: Recorded = Default::default();
    let sink = calls.clone();
    let runner: crate::app::new_studio::NsRun = Arc::new(move |machine, argv, _stdin| {
        sink.lock().unwrap().push((machine, argv.clone()));
        let is_show = argv.windows(2).any(|w| w == ["report", "show"]);
        Box::pin(async move {
            if is_show {
                Ok(show_body.to_owned())
            } else {
                Ok(String::new())
            }
        })
    });
    (calls, runner)
}

fn design_app(suggest_skip: bool, include_design: bool) -> App {
    let mut app = app_with_hollywood();
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = crate::app::pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    let mut doc = summary_json();
    doc["hollywood"]["cards"] = serde_json::json!([]);
    doc["studios"][0]["id"] = serde_json::json!("beta");
    doc["studios"][0]["title"] = serde_json::json!("Beta");
    doc["studios"][0]["waiting"] = serde_json::json!(0);
    doc["studios"][0]["roles"] = serde_json::json!([
        { "role": "producer", "name": "producer", "terminal": "t1",
          "state": "live", "work": "waiting" }
    ]);
    if include_design {
        doc["studios"][0]["design"] = if suggest_skip {
            serde_json::json!({
                "version": 1,
                "round": 0,
                "phase": "suggest_skip",
                "waiting_user": true,
                "suggest_skip": true,
                "skip_reason": "config flag only; no architecture tradeoffs",
                "doc_file": "design/v1.md",
                "questions": [],
                "waiting_since_ms": 940_000
            })
        } else {
            serde_json::json!({
                "version": 1,
                "round": 2,
                "phase": "waiting_user",
                "waiting_user": true,
                "suggest_skip": false,
                "doc_file": "design/v1.md",
                "summary": "Keep single binary with SQLite WAL or split worker crate",
                "questions": [
                    {
                        "id": "Q1",
                        "prompt": "Separate job worker crate?",
                        "recommendation": "single crate",
                        "section_ref": "§R2.W"
                    },
                    {
                        "id": "Q2",
                        "prompt": "Storage backend?",
                        "recommendation": "SQLite WAL",
                        "section_ref": "§R2.D"
                    }
                ],
                "waiting_since_ms": 940_000
            })
        };
    }
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.studio.current = Some(StudioKey::local("beta"));
    app.panel.mode = PanelMode::Expanded;
    app.shell_state.sidebar_collapsed = true;
    app.reconcile_published_view();
    app
}

fn select_decision_in_sheet(app: &mut App) {
    app.area = (200, 44);
    if app.hollywood.mode != LayerMode::Expanded {
        alt_y(app);
    }
    draw_to(app, 200, 44);
    if !app
        .hollywood
        .selected
        .as_ref()
        .is_some_and(|t| format!("{t:?}").contains("Decision"))
    {
        key(app, hc_vt::Key::BackTab);
    }
    assert!(
        app.hollywood
            .selected
            .as_ref()
            .is_some_and(|t| format!("{t:?}").contains("Decision")),
        "expected Call Sheet Decision target, got {:?}",
        app.hollywood.selected
    );
}

fn select_design_in_dock(app: &mut App) {
    app.area = (200, 44);
    app.hollywood.mode = LayerMode::Collapsed;
    app.panel.mode = PanelMode::Expanded;
    app.panel.focused = true;
    draw_to(app, 200, 44);
    let idx = app
        .panel_targets()
        .into_iter()
        .position(|t| format!("{t:?}").contains("Design"))
        .expect("Lot Dock missing Target::Design");
    let target = app.panel_targets()[idx].clone();
    app.panel.selected = Some(target);
    app.panel.selected_row = Some(idx);
}

#[tokio::test]
async fn callsheet_and_dock_200x44_and_100x40_render_pill_decision_card_and_strip() {
    for (cols, rows) in [(200u16, 44u16), (100u16, 40u16)] {
        let mut app = design_app(false, true);
        app.area = (cols, rows);
        app.panel.mode = PanelMode::Expanded;
        app.hollywood.mode = LayerMode::Collapsed;
        let dock_buf = draw_to(&mut app, cols, rows);
        let dock_screen = screen_lines(&dock_buf).join("\n");
        let top_bar = dock_screen.lines().next().unwrap_or_default();
        assert!(
            top_bar.contains("?1"),
            "{cols}x{rows} missing ?1 pill:\n{top_bar}"
        );
        assert!(
            !top_bar.contains("!1"),
            "{cols}x{rows} must not show !1:\n{top_bar}"
        );
        let pill = app.hit_map.lot_pill;
        let q_x = (pill.x..pill.right())
            .find(|&x| dock_buf.buffer()[(x, pill.y)].symbol() == "?")
            .expect("? pill cell inside lot_pill");
        assert_eq!(
            dock_buf.buffer()[(q_x, pill.y)].fg,
            app.shell.palette.peach,
            "{cols}x{rows} ?1 pill must use warm peach"
        );
        assert_ne!(
            dock_buf.buffer()[(pill.x + 1, pill.y)].bg,
            app.shell.palette.red,
            "{cols}x{rows} pill must never turn red without blocking alerts"
        );
        assert!(
            dock_screen.contains("◆ design · R2 · you"),
            "{cols}x{rows} Lot Dock missing design header:\n{dock_screen}"
        );
        assert!(
            dock_screen.contains("r reply   o read   a sign"),
            "{cols}x{rows} Lot Dock missing footer:\n{dock_screen}"
        );

        // Collapsed 16-col strip
        app.panel.mode = PanelMode::Collapsed;
        let strip_buf = draw_to(&mut app, cols, rows);
        let strip_screen = screen_lines(&strip_buf).join("\n");
        assert!(
            strip_screen.contains("◆R2 ?") || strip_screen.contains("◆?"),
            "{cols}x{rows} collapsed strip missing `◆R2 ?`:\n{strip_screen}"
        );

        // Expanded Call Sheet with Decision selected
        app.panel.mode = PanelMode::Expanded;
        app.hollywood.mode = LayerMode::Collapsed;
        alt_y(&mut app);
        draw_to(&mut app, cols, rows);
        key(&mut app, hc_vt::Key::BackTab);
        let sheet_buf = draw_to(&mut app, cols, rows);
        let sheet_screen = screen_lines(&sheet_buf).join("\n");
        if std::env::var_os("DESIGN_CARD_DUMP").is_some() {
            eprintln!("=== DOCK {cols}x{rows} ===\n{dock_screen}\n=== END DOCK {cols}x{rows} ===");
            eprintln!(
                "=== STRIP {cols}x{rows} ===\n{strip_screen}\n=== END STRIP {cols}x{rows} ==="
            );
            eprintln!(
                "=== SHEET {cols}x{rows} ===\n{sheet_screen}\n=== END SHEET {cols}x{rows} ==="
            );
        }
        assert!(
            sheet_screen.contains("DECISION"),
            "{cols}x{rows} Call Sheet missing DECISION:\n{sheet_screen}"
        );
        assert!(
            sheet_screen.contains("Q1") && sheet_screen.contains("§R2.W"),
            "{cols}x{rows} Call Sheet missing Q1 / §R2.W:\n{sheet_screen}"
        );
    }
}

#[tokio::test]
async fn zero_design_invariance_at_200x44_and_100x40() {
    for (cols, rows) in [(200u16, 44u16), (100u16, 40u16)] {
        let mut a = design_app(false, false);
        let mut b = design_app(false, false);
        a.area = (cols, rows);
        b.area = (cols, rows);
        assert_eq!(
            draw_to(&mut a, cols, rows).buffer(),
            draw_to(&mut b, cols, rows).buffer()
        );
    }
}

#[tokio::test]
async fn callsheet_decision_keys_a_confirm_cancel_r_reply_suggest_skip_open_and_o_read() {
    let mut app = design_app(false, true);
    let (calls, runner) = ns_recorder("# Design discussion · req v1\n\n## R2\nProposed monolith.");
    app.set_ns_run(runner);
    select_decision_in_sheet(&mut app);

    // 1. `a` arms confirmation showing `sign off` + file without sending anything yet.
    key(&mut app, hc_vt::Key::Char('a'));
    assert!(calls.lock().unwrap().is_empty(), "`a` must confirm first");
    assert!(
        app.studio_dialog.is_some(),
        "expected confirm dialog on `a`"
    );
    let dialog_text = screen_lines(&draw_to(&mut app, 200, 44)).join("\n");
    assert!(
        dialog_text.contains("sign off") && dialog_text.contains("design/v1-final.md"),
        "confirm dialog must show mode and file:\n{dialog_text}"
    );
    // Cancel sends nothing.
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.studio_dialog.is_none());
    assert!(calls.lock().unwrap().is_empty(), "cancel must send nothing");

    // Re-arm and confirm (`y`) runs `design sign`.
    key(&mut app, hc_vt::Key::Char('a'));
    key(&mut app, hc_vt::Key::Char('y'));
    let sign_reply = app.action_rx.recv().await.expect("sign reply");
    app.on_action_reply(sign_reply);

    // 2. `r` on WaitingUser focuses compose reply to producer.
    key(&mut app, hc_vt::Key::Char('r'));
    assert!(app.hollywood.editing);
    assert_eq!(
        app.hollywood.reply_to,
        Some((StudioKey::local("beta"), "producer".to_string()))
    );
    for ch in "use wal".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    let tell_reply = app.action_rx.recv().await.expect("tell reply");
    app.on_action_reply(tell_reply);

    // 3. `o` opens PR 6's Design Reader (`ReaderKind::Design` on `design/v1.md`),
    // where `a` opens the exact same confirmation dialog.
    select_decision_in_sheet(&mut app);
    key(&mut app, hc_vt::Key::Char('o'));
    let show_reply = app.action_rx.recv().await.expect("show reply");
    app.on_action_reply(show_reply);
    let view = app.hollywood.view.as_ref().expect("`o` must open Reader");
    assert_eq!(view.meta.kind, crate::hollywood::ReaderKind::Design);
    assert_eq!(view.meta.file.as_deref(), Some("design/v1.md"));
    assert!(view.raw_body.contains("Proposed monolith."), "{view:?}");
    key(&mut app, hc_vt::Key::Char('a'));
    let reader_confirm = screen_lines(&draw_to(&mut app, 200, 44)).join("\n");
    assert!(
        reader_confirm.contains("sign off") && reader_confirm.contains("design/v1-final.md"),
        "Design Reader `a` must open the same confirmation dialog:\n{reader_confirm}"
    );
    key(&mut app, hc_vt::Key::Escape);
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.hollywood.view.is_none());

    // 4. On `SuggestSkip`: `a` shows `skip` and runs `design sign --skip`, `r` runs `design open`.
    let mut skip_app = design_app(true, true);
    let (skip_calls, skip_runner) = ns_recorder("# Design discussion · req v1\n");
    skip_app.set_ns_run(skip_runner);
    select_decision_in_sheet(&mut skip_app);
    key(&mut skip_app, hc_vt::Key::Char('a'));
    let skip_dialog = screen_lines(&draw_to(&mut skip_app, 200, 44)).join("\n");
    assert!(
        skip_dialog.contains("skip") && skip_dialog.contains("design/v1-final.md"),
        "skip confirm must show mode + file:\n{skip_dialog}"
    );
    key(&mut skip_app, hc_vt::Key::Enter);
    let s1 = skip_app.action_rx.recv().await.expect("skip reply");
    skip_app.on_action_reply(s1);
    select_decision_in_sheet(&mut skip_app);
    key(&mut skip_app, hc_vt::Key::Char('r'));
    let s2 = skip_app.action_rx.recv().await.expect("open reply");
    skip_app.on_action_reply(s2);

    assert_eq!(
        calls.lock().unwrap().as_slice(),
        &[
            (
                None,
                vec![
                    "design".into(),
                    "sign".into(),
                    "--studio".into(),
                    "beta".into(),
                    "--as".into(),
                    "user".into()
                ]
            ),
            (
                None,
                vec![
                    "tell".into(),
                    "producer".into(),
                    "--text".into(),
                    "use wal".into(),
                    "--studio".into(),
                    "beta".into(),
                    "--as".into(),
                    "user".into()
                ]
            ),
            (
                None,
                vec![
                    "report".into(),
                    "show".into(),
                    "--file".into(),
                    "design/v1.md".into(),
                    "--studio".into(),
                    "beta".into(),
                    "--as".into(),
                    "user".into()
                ]
            ),
        ]
    );
    assert_eq!(
        skip_calls.lock().unwrap().as_slice(),
        &[
            (
                None,
                vec![
                    "design".into(),
                    "sign".into(),
                    "--skip".into(),
                    "--studio".into(),
                    "beta".into(),
                    "--as".into(),
                    "user".into()
                ]
            ),
            (
                None,
                vec![
                    "design".into(),
                    "open".into(),
                    "--studio".into(),
                    "beta".into(),
                    "--as".into(),
                    "user".into()
                ]
            ),
        ]
    );
}

#[tokio::test]
async fn dock_decision_keys_r_o_a_and_focused_footer_hints() {
    let mut app = design_app(false, true);
    let (calls, runner) = ns_recorder("# Design discussion · req v1\n\n## R2");
    app.set_ns_run(runner);
    select_design_in_dock(&mut app);

    let focused = screen_lines(&draw_to(&mut app, 200, 44)).join("\n");
    assert!(
        focused.contains("o read") && focused.contains("a sign") && focused.contains("r reply"),
        "focused dock footer missing design hints:\n{focused}"
    );

    key(&mut app, hc_vt::Key::Char('a'));
    assert!(app.studio_dialog.is_some());
    key(&mut app, hc_vt::Key::Char('n'));
    assert!(app.studio_dialog.is_none());
    assert!(calls.lock().unwrap().is_empty());

    key(&mut app, hc_vt::Key::Char('a'));
    key(&mut app, hc_vt::Key::Char('y'));
    let r1 = app.action_rx.recv().await.expect("dock sign");
    app.on_action_reply(r1);

    key(&mut app, hc_vt::Key::Char('o'));
    let r2 = app.action_rx.recv().await.expect("dock show");
    app.on_action_reply(r2);
    let dock_view = app.hollywood.view.as_ref().expect("dock `o` opens Reader");
    assert_eq!(dock_view.meta.kind, crate::hollywood::ReaderKind::Design);
    key(&mut app, hc_vt::Key::Escape);

    key(&mut app, hc_vt::Key::Char('r'));
    assert!(app.panel.editing, "dock `r` focuses studio compose");
    for ch in "prefer sqlite".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    let r3 = app.action_rx.recv().await.expect("dock tell");
    app.on_action_reply(r3);

    assert_eq!(
        calls.lock().unwrap().as_slice(),
        &[
            (
                None,
                vec![
                    "design".into(),
                    "sign".into(),
                    "--studio".into(),
                    "beta".into(),
                    "--as".into(),
                    "user".into()
                ]
            ),
            (
                None,
                vec![
                    "report".into(),
                    "show".into(),
                    "--file".into(),
                    "design/v1.md".into(),
                    "--studio".into(),
                    "beta".into(),
                    "--as".into(),
                    "user".into()
                ]
            ),
            (
                None,
                vec![
                    "tell".into(),
                    "producer".into(),
                    "--text".into(),
                    "prefer sqlite".into(),
                    "--studio".into(),
                    "beta".into(),
                    "--as".into(),
                    "user".into()
                ]
            ),
        ]
    );
}

#[tokio::test]
async fn callsheet_decision_enter_opens_show_view_with_verbatim_checks() {
    let mut app = design_app(false, true);
    let (calls, runner) = ns_recorder("# Design discussion\n\nProposed monolith.");
    app.set_ns_run(runner);
    let mut doc = app
        .view
        .documents_mut()
        .get(&doc_key())
        .unwrap()
        .value
        .clone();
    doc["studios"][0]["design"] = serde_json::json!({
        "version": 1,
        "round": 2,
        "phase": "waiting_user",
        "waiting_user": true,
        "suggest_skip": false,
        "doc_file": "design/v1.md",
        "summary": "Pick store backend",
        "questions": [],
        "proposed_checks": [
            {
                "name": "scan",
                "run": "sh scripts/scan.sh --strict",
                "timeout_s": 45,
                "expect": "files_scanned=[1-9]"
            },
            {
                "name": "lint",
                "run": "cargo clippy --all-targets -- -D warnings",
                "timeout_s": 120
            }
        ],
        "waiting_since_ms": 940_000
    });
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.studio.current = Some(StudioKey::local("beta"));
    app.reconcile_published_view();

    select_decision_in_sheet(&mut app);
    let sheet_text = screen_lines(&draw_to(&mut app, 200, 44)).join("\n");
    assert!(
        sheet_text.contains("+2 checks"),
        "Call Sheet DECISION row missing `+2 checks`:\n{sheet_text}"
    );

    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        app.hollywood.mode,
        LayerMode::Expanded,
        "Enter on DECISION with checks must open show view, not leave Call Sheet"
    );
    let view = app
        .hollywood
        .view
        .as_ref()
        .expect("Enter on DECISION must open CardView show view");
    assert!(
        view.raw_body
            .starts_with("Signing off also activates 2 checks:"),
        "show view must start with `Signing off also activates 2 checks:`, got:\n{}",
        view.raw_body
    );

    let show_screen = screen_lines(&draw_to(&mut app, 200, 44)).join("\n");
    if std::env::var_os("DECISION_CHECKS_DUMP").is_some() {
        eprintln!(
            "=== CALL SHEET (200x44) ===\n{sheet_text}\n=== DECISION SHOW VIEW (200x44) ===\n{show_screen}"
        );
    }
    for needle in [
        "Signing off also activates 2 checks:",
        "scan",
        "sh scripts/scan.sh --strict",
        "45s",
        "files_scanned=[1-9]",
        "lint",
        "cargo clippy --all-targets -- -D warnings",
        "120s",
    ] {
        assert!(
            show_screen.contains(needle),
            "DECISION show view missing verbatim `{needle}`:\n{show_screen}"
        );
    }

    // Pressing `a` inside the DECISION show view opens the 3-choice sign modal
    // displaying both checks verbatim.
    key(&mut app, hc_vt::Key::Char('a'));
    let confirm_screen = screen_lines(&draw_to(&mut app, 200, 44)).join("\n");
    if std::env::var_os("DECISION_CHECKS_DUMP").is_some() {
        eprintln!("=== SIGN MODAL (200x44) ===\n{confirm_screen}\n=== END ===");
    }
    for want in [
        "scan (timeout 45s)",
        "run: sh scripts/scan.sh --strict",
        "expect: files_scanned=[1-9]",
        "lint (timeout 120s)",
        "run: cargo clippy --all-targets -- -D warnings",
        "Sign off and activate 2 checks",
        "Sign off without checks (--no-checks)",
        "Cancel",
    ] {
        assert!(
            confirm_screen.contains(want),
            "sign modal missing `{want}`:\n{confirm_screen}"
        );
    }

    // Choosing "Cancel" (`ArrowDown` twice + `Enter`) sends nothing.
    key(&mut app, hc_vt::Key::ArrowDown);
    key(&mut app, hc_vt::Key::ArrowDown);
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.studio_dialog.is_none());
    assert!(calls.lock().unwrap().is_empty(), "Cancel must send nothing");

    // Re-open `a`, move to "Sign off without checks (--no-checks)" (`ArrowDown` + `Enter`)
    // -> runs `backlot design sign --no-checks`.
    key(&mut app, hc_vt::Key::Char('a'));
    key(&mut app, hc_vt::Key::ArrowDown);
    key(&mut app, hc_vt::Key::Enter);
    let no_checks_reply = app.action_rx.recv().await.expect("no-checks sign reply");
    app.on_action_reply(no_checks_reply);
    {
        let recorded = calls.lock().unwrap();
        let argv = &recorded.last().expect("recorded sign call").1;
        assert!(
            argv.starts_with(&[
                "design".to_string(),
                "sign".to_string(),
                "--no-checks".to_string()
            ]),
            "expected `design sign --no-checks`, got {argv:?}"
        );
    }

    // Re-open `a`, confirm default ("Sign off and activate 2 checks") -> omits `--no-checks`.
    calls.lock().unwrap().clear();
    key(&mut app, hc_vt::Key::Char('a'));
    key(&mut app, hc_vt::Key::Enter);
    let with_checks_reply = app.action_rx.recv().await.expect("with-checks sign reply");
    app.on_action_reply(with_checks_reply);
    {
        let recorded = calls.lock().unwrap();
        let argv = &recorded.last().expect("recorded sign call").1;
        assert!(
            argv.starts_with(&["design".to_string(), "sign".to_string()])
                && !argv.iter().any(|a| a == "--no-checks"),
            "expected plain `design sign` with checks, got {argv:?}"
        );
    }
}

#[tokio::test]
async fn show_view_and_reader_wrap_150_char_run_at_80_columns() {
    let long_run = format!("check_{}_end", "x".repeat(140));
    assert_eq!(long_run.len(), 150);

    let mut app = design_app(false, true);
    let (_calls, runner) = ns_recorder("# Design\n\nBody.");
    app.set_ns_run(runner);
    let mut doc = app
        .view
        .documents_mut()
        .get(&doc_key())
        .unwrap()
        .value
        .clone();
    doc["studios"][0]["design"] = serde_json::json!({
        "version": 1,
        "round": 2,
        "phase": "waiting_user",
        "waiting_user": true,
        "suggest_skip": false,
        "doc_file": "design/v1.md",
        "summary": "Pick store backend",
        "questions": [],
        "proposed_checks": [
            {
                "name": "long_scan",
                "run": long_run,
                "timeout_s": 45,
                "expect": "files_scanned=[1-9]"
            }
        ],
        "waiting_since_ms": 940_000
    });
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.studio.current = Some(StudioKey::local("beta"));
    app.reconcile_published_view();

    select_decision_in_sheet(&mut app);
    key(&mut app, hc_vt::Key::Enter);
    let screen_80 = screen_lines(&draw_to(&mut app, 80, 44)).join("\n");
    if std::env::var_os("DECISION_CHECKS_DUMP").is_some() {
        eprintln!("=== DECISION SHOW VIEW (80x44, 150-char run) ===\n{screen_80}\n=== END ===");
    }
    let flatten_code_block = |screen: &str| -> String {
        screen
            .lines()
            .filter_map(|l| {
                let (_, after_gutter) = l.split_once('▏')?;
                let inner = after_gutter
                    .rsplit_once('│')
                    .map_or(after_gutter, |(s, _)| s);
                Some(inner.trim())
            })
            .collect::<Vec<_>>()
            .join("")
    };
    let flattened = flatten_code_block(&screen_80);
    assert!(
        flattened.contains(&long_run),
        "150-char `run` must wrap cleanly in full at 80 columns:\n{screen_80}"
    );

    // Close show view, open `[o]` Design Reader at 80x44 -> also wraps 150-char `run`
    // in full, and pressing `a` opens the shared sign modal wrapping it at 80x44 too.
    key(&mut app, hc_vt::Key::Escape);
    select_decision_in_sheet(&mut app);
    key(&mut app, hc_vt::Key::Char('o'));
    let show_reply = app.action_rx.recv().await.expect("show reply");
    app.on_action_reply(show_reply);
    let reader_80 = screen_lines(&draw_to(&mut app, 80, 44)).join("\n");
    let reader_flat = flatten_code_block(&reader_80);
    assert!(
        reader_flat.contains(&long_run),
        "Design Reader `[o]` must wrap 150-char `run` cleanly in full at 80 columns:\n{reader_80}"
    );
    key(&mut app, hc_vt::Key::Char('a'));
    let modal_80 = screen_lines(&draw_to(&mut app, 80, 44)).join("\n");
    let modal_area = app.studio_dialog_hits.area;
    let modal_flat: String = modal_80
        .lines()
        .skip(modal_area.y as usize)
        .take(modal_area.height as usize)
        .filter_map(|l| {
            let start = usize::from(modal_area.x);
            let chars: String = l
                .chars()
                .skip(start)
                .take(usize::from(modal_area.width))
                .collect();
            Some(
                chars
                    .trim()
                    .trim_matches(|c: char| {
                        c == '│' || c == '╭' || c == '╮' || c == '╰' || c == '╯' || c == '─'
                    })
                    .trim()
                    .to_owned(),
            )
        })
        .collect::<Vec<_>>()
        .join("");
    assert!(
        modal_flat.contains(&long_run),
        "Sign modal opened from Design Reader must wrap 150-char `run` cleanly at 80 columns:\n{modal_80}"
    );
    key(&mut app, hc_vt::Key::Escape);
}

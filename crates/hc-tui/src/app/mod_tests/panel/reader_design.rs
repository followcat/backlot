//! Fail-first App wiring tests for PR 6 Design Reader: `[`/`]` section jumping,
//! `r` reply to producer, `a` sign-off confirm dispatch (`backlot design sign`),
//! finalized read-only protection, `200x44` / `100x40` dumps, and no-design identity.

use super::*;
use crate::hollywood::{CardView, ReaderKind, ReaderMeta, ReaderOrigin};
use crate::studio::StudioKey;
use backlot_model::design::{DesignPhase, SignMode};

fn design_body_lines() -> Vec<String> {
    let mut lines = vec![
        "# Design v1 · acme-shop".to_string(),
        String::new(),
        "## R1".to_string(),
        String::new(),
        "### §R1.W Proposal · writer (#11)".to_string(),
        String::new(),
    ];
    for i in 0..18 {
        lines.push(format!(
            "Proposal detail paragraph line {i}: validate section jump scroll offsets cleanly."
        ));
    }
    lines.extend([
        String::new(),
        "### §R1.D Buildability · director (#12)".to_string(),
        String::new(),
    ]);
    for i in 0..18 {
        lines.push(format!(
            "Director buildability note {i}: ensure buffer tests cover 200x44 and 100x40."
        ));
    }
    lines.extend([
        String::new(),
        "## R2".to_string(),
        String::new(),
        "### §R2.P Summary · producer (#13)".to_string(),
        String::new(),
        "Producer summary waiting for user decision.".to_string(),
    ]);
    lines
}

fn open_design_in(app: &mut App, phase: DesignPhase, file: &str, finalized: bool) {
    let lines = design_body_lines();
    app.hollywood.view = Some(CardView {
        card: None,
        title: "Design".into(),
        raw_body: lines.join("\n"),
        lines,
        scroll: 0,
        meta: ReaderMeta {
            kind: ReaderKind::Design,
            studio: Some("acme-shop".into()),
            file: Some(file.into()),
            design_version: Some(1),
            design_round: Some(2),
            design_phase: Some(phase),
            design_sign_mode: finalized.then_some(SignMode::Approved),
            origin: ReaderOrigin::Design(StudioKey::local("acme-shop")),
            ..ReaderMeta::default()
        },
        ..CardView::default()
    });
}

#[tokio::test]
async fn design_reader_section_jump_reply_and_sign_off_work_end_to_end() {
    let (recorded, r) = runner();
    let mut app = app_with_panel_doc();
    app.set_panel_runner(r);
    app.area = (120, 32);

    open_design_in(&mut app, DesignPhase::WaitingUser, "design/v1.md", false);
    let initial_backend = draw_to(&mut app, 120, 32);
    let initial_text = screen_lines(&initial_backend).join("\n");
    assert!(
        initial_text.contains("DESIGN v1"),
        "expected DESIGN v1 header:\n{initial_text}"
    );
    assert!(
        initial_text.contains("sign off"),
        "expected sign off chip when WaitingUser:\n{initial_text}"
    );

    // Jump forward across sections with `]` then back with `[`.
    key(&mut app, hc_vt::Key::Char(']'));
    let _ = draw_to(&mut app, 120, 32);
    key(&mut app, hc_vt::Key::Char(']'));
    let jumped_backend = draw_to(&mut app, 120, 32);
    let jumped_scroll = app.hollywood.view.as_ref().unwrap().scroll;
    assert!(
        jumped_scroll > 0,
        "pressing `]` twice should scroll to §R1.D (got scroll={jumped_scroll})"
    );
    let jumped_text = screen_lines(&jumped_backend).join("\n");
    assert!(
        jumped_text.contains("§R1.D") || jumped_text.contains("§R1.W"),
        "header should reflect current section after jump:\n{jumped_text}"
    );
    key(&mut app, hc_vt::Key::Char('['));
    let back_scroll = app.hollywood.view.as_ref().unwrap().scroll;
    assert!(
        back_scroll < jumped_scroll,
        "pressing `[` should jump back to earlier section ({back_scroll} vs {jumped_scroll})"
    );

    // Pressing `r` opens compose targeting `producer`, typing and submitting dispatches `backlot tell producer`.
    key(&mut app, hc_vt::Key::Char('r'));
    assert!(
        app.hollywood.view.as_ref().unwrap().tell.is_composing(),
        "pressing `r` in Design Reader must open inline reply compose"
    );
    let compose_backend = draw_to(&mut app, 120, 32);
    let compose_text = screen_lines(&compose_backend).join("\n");
    assert!(
        compose_text.contains("producer"),
        "reply compose should target producer:\n{compose_text}"
    );
    for ch in "split step two".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    tokio::task::yield_now().await;
    {
        let argvs = recorded.lock().unwrap();
        assert!(
            argvs.iter().any(|argv| {
                argv.iter().any(|a| a == "tell")
                    && argv.iter().any(|a| a == "producer")
                    && argv.iter().any(|a| a == "split step two")
            }),
            "submitting `r` reply in Design Reader must target `producer`, got: {argvs:?}"
        );
    }
    recorded.lock().unwrap().clear();

    // Pressing `a` triggers sign-off confirm dialog, then `y` runs `backlot design sign`.
    key(&mut app, hc_vt::Key::Char('a'));
    assert!(
        app.studio_dialog.is_some(),
        "pressing `a` in waiting Design Reader must open sign-off confirmation dialog"
    );
    let confirm_backend = draw_to(&mut app, 120, 32);
    let confirm_text = screen_lines(&confirm_backend).join("\n");
    assert!(
        confirm_text.contains("sign off") || confirm_text.contains("Sign off"),
        "confirm dialog should describe design sign off:\n{confirm_text}"
    );
    key(&mut app, hc_vt::Key::Char('y'));
    tokio::task::yield_now().await;

    let argvs = recorded.lock().unwrap().clone();
    assert!(
        argvs.iter().any(|argv| {
            argv.iter().any(|a| a == "design")
                && argv.iter().any(|a| a == "sign")
                && argv.iter().any(|a| a == "acme-shop")
        }),
        "confirming design sign-off must dispatch `backlot design sign`, got: {argvs:?}"
    );

    // Finalized document must ignore `a` and omit sign off.
    recorded.lock().unwrap().clear();
    open_design_in(&mut app, DesignPhase::Signed, "design/v1-final.md", true);
    let final_backend = draw_to(&mut app, 120, 32);
    let final_text = screen_lines(&final_backend).join("\n");
    assert!(
        !final_text.contains("sign off"),
        "signed design document must not render sign off chip:\n{final_text}"
    );
    key(&mut app, hc_vt::Key::Char('a'));
    assert!(
        app.studio_dialog.is_none(),
        "pressing `a` on finalized design doc must not open sign-off dialog"
    );

    // Opening via `studio_cmds::design_show` + `on_view_action_reply` populates `ReaderKind::Design`
    // and `plan/*.md` opens cleanly as Markdown too.
    let key_acme = StudioKey::local("acme-shop");
    let show_design = crate::studio_cmds::design_show(&key_acme, "design/v1.md");
    app.on_view_action_reply(&show_design, &design_body_lines().join("\n"));
    assert_eq!(
        app.hollywood.view.as_ref().unwrap().meta.kind,
        ReaderKind::Design
    );
    let show_plan = crate::studio_cmds::design_show(&key_acme, "plan/v1.md");
    app.on_view_action_reply(&show_plan, "# Plan v1\n\nStep 1.\n");
    assert_eq!(
        app.hollywood.view.as_ref().unwrap().meta.file.as_deref(),
        Some("plan/v1.md")
    );

    // SuggestSkip + `a` + `y` dispatches `--skip`.
    recorded.lock().unwrap().clear();
    open_design_in(&mut app, DesignPhase::SuggestSkip, "design/v1.md", false);
    key(&mut app, hc_vt::Key::Char('a'));
    key(&mut app, hc_vt::Key::Char('y'));
    tokio::task::yield_now().await;
    let skip_argvs = recorded.lock().unwrap().clone();
    assert!(
        skip_argvs.iter().any(|argv| {
            argv.iter().any(|a| a == "design")
                && argv.iter().any(|a| a == "sign")
                && argv.iter().any(|a| a == "--skip")
        }),
        "SuggestSkip confirmation must dispatch `--skip`, got: {skip_argvs:?}"
    );
}

#[tokio::test]
async fn design_reader_200x44_and_100x40_and_no_design_identity() {
    let mut app_without = app_with_panel_doc();
    app_without.area = (200, 44);
    open_panel(&mut app_without);
    let base_200 = screen_lines(&draw_to(&mut app_without, 200, 44)).join("\n");
    app_without.area = (100, 40);
    let base_100 = screen_lines(&draw_to(&mut app_without, 100, 40)).join("\n");
    assert!(!base_200.contains("DESIGN") && !base_200.contains("sign off"));
    assert!(!base_100.contains("DESIGN") && !base_100.contains("sign off"));

    let mut app = app_with_panel_doc();
    open_design_in(&mut app, DesignPhase::WaitingUser, "design/v1.md", false);
    app.area = (200, 44);
    let dump_200 = screen_lines(&draw_to(&mut app, 200, 44)).join("\n");
    app.area = (100, 40);
    let dump_100 = screen_lines(&draw_to(&mut app, 100, 40)).join("\n");

    assert!(dump_200.contains("DESIGN v1"), "{dump_200}");
    assert!(dump_200.contains("sign off"), "{dump_200}");
    assert!(dump_100.contains("DESIGN v1"), "{dump_100}");
    assert!(dump_100.contains("sign off"), "{dump_100}");
}

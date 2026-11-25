//! `m` on a selected band target — the row's context menu must open
//! AND paint (the sim's invisible-menu bug): the frame's menu anchors
//! at its rect, a band row's at its row, and a target the last frame
//! never drew falls back to the band's corner — never nowhere.
use super::*;
use crate::chrome::menu::ContextMenuTarget;

/// Select the finished `beta-tools` frame, press `m`: the studio's
/// menu opens with its items, and the next draw paints the box —
/// `m` must never leave the keys in a menu-less dead end.
#[tokio::test]
async fn m_on_a_studio_frame_opens_a_visible_menu() {
    let mut app = app_with_hollywood();
    app.area = (120, 30);
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    draw_to(&mut app, 120, 30);
    app.hollywood_select(Target::Studio(crate::studio::StudioKey {
        machine: None,
        id: "beta-tools".into(),
    }));
    key(&mut app, hc_vt::Key::Char('m'));
    let Some(menu) = app.menu.as_ref() else {
        panic!("m on a studio frame opens its menu");
    };
    assert!(
        matches!(menu.target, ContextMenuTarget::Studio(_)),
        "a frame's menu is the studio's"
    );
    let text = screen_lines(&draw_to(&mut app, 120, 30)).join("\n");
    assert!(
        text.contains("Inspect studio sheet"),
        "the menu paints: {text}"
    );
}

/// A prompt row's menu. `actor-alice` gets `attention_user` so the
/// row exists in `hollywood_targets` — a target that is not in the
/// list silently selects index 0 (the title), which opens nothing.
#[tokio::test]
async fn m_on_a_waiting_row_opens_its_menu() {
    let mut app = app_with_hollywood();
    let mut doc = summary_json();
    doc["studios"][0]["roles"][1]["attention_user"] = serde_json::json!(true);
    doc["studios"][0]["roles"][1]["attention_seq"] = serde_json::json!(7);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.area = (120, 30);
    alt_y(&mut app);
    draw_to(&mut app, 120, 30);
    let target = app
        .hollywood_targets()
        .into_iter()
        .find(|t| matches!(t, Target::Prompt(_, role) if role == "actor-alice"))
        .expect("the prompt row is a target");
    app.hollywood_select(target);
    key(&mut app, hc_vt::Key::Char('m'));
    let Some(menu) = app.menu.as_ref() else {
        panic!("a prompt row's menu opens");
    };
    assert!(
        matches!(menu.target, ContextMenuTarget::Studio(_)),
        "the menu is the role's"
    );
    let text = screen_lines(&draw_to(&mut app, 120, 30)).join("\n");
    assert!(text.contains("Allow prompt"), "the menu paints: {text}");
}

/// `m` on a target with no menu (`Target::Input`, a real band target)
/// opens nothing — and the keys stay with the band, not a phantom
/// layer. Esc still collapses afterwards.
#[tokio::test]
async fn m_on_a_menu_less_target_opens_nothing_and_esc_still_collapses() {
    let mut app = app_with_hollywood();
    app.area = (120, 30);
    alt_y(&mut app);
    draw_to(&mut app, 120, 30);
    // `Alt+Y` lands in Compose on `Target::Input`; one Esc rests there.
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert!(!app.hollywood.editing);
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(app.menu.is_none(), "the input row has no menu");
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
}

#[tokio::test]
async fn m_on_title_opens_menu_to_change_hollywood_agent_and_applies_setup() {
    use super::super::delete_studio::drain_dialog_fetch;
    use super::super::lifecycle_fixtures::RUNNERS_JSON;
    use crate::studio_dialog::{RecastStep, StudioDialog};

    let mut app = app_with_hollywood();
    app.set_hollywood(
        Ok(Some(HollywoodCfg {
            runner: "claude".into(),
            model: "m2".into(),
            effort: Some("high".into()),
            key: Some("alt+y".into()),
            default_cast: Some("/home/user/cast.toml".into()),
        })),
        std::path::Path::new("config.toml"),
    );
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_ns_run(std::sync::Arc::new(move |_machine, argv, _stdin| {
        sink.lock().unwrap().push(argv.clone());
        let out = if argv.first().map(String::as_str) == Some("runners") {
            Ok(RUNNERS_JSON.to_owned())
        } else {
            Ok("[hollywood] written".to_owned())
        };
        Box::pin(async move { out })
    }));

    app.area = (120, 30);
    alt_y(&mut app);
    draw_to(&mut app, 120, 30);
    // Navigate via real keys: Esc leaves Compose, Tab -> Waiting, Tab -> Studios, ArrowUp -> Title.
    key(&mut app, hc_vt::Key::Escape);
    key(&mut app, hc_vt::Key::Tab);
    key(&mut app, hc_vt::Key::Tab);
    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(app.hollywood.selected, Some(Target::Title));
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(app.menu.is_some(), "`m` on Title opens the title menu");
    let text = screen_lines(&draw_to(&mut app, 120, 30)).join("\n");
    assert!(text.contains("Change Hollywood's agent…"), "{text}");

    key(&mut app, hc_vt::Key::Char('c'));
    assert!(matches!(app.studio_dialog, Some(StudioDialog::Recast(_))));
    drain_dialog_fetch(&mut app).await;
    {
        let Some(StudioDialog::Recast(d)) = &app.studio_dialog else {
            panic!("expected Recast dialog");
        };
        assert!(d.key.is_none());
        assert_eq!(d.runner_name(), Some("claude"));
        assert_eq!(d.selected_model().as_deref(), Some("m2"));
        assert_eq!(d.selected_effort().as_deref(), Some("high"));
    }

    // Move to `cursor`, choose `auto`, skip effort (none), apply.
    key(&mut app, hc_vt::Key::ArrowDown);
    key(&mut app, hc_vt::Key::Enter);
    key(&mut app, hc_vt::Key::Enter);
    {
        let Some(StudioDialog::Recast(d)) = &app.studio_dialog else {
            panic!("expected Recast apply step");
        };
        assert_eq!(d.step, RecastStep::Apply);
    }
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.studio_dialog.is_none());

    {
        let calls = recorded.lock().unwrap();
        assert_eq!(
            calls.last().unwrap(),
            &vec![
                "hollywood".to_string(),
                "setup".to_string(),
                "--runner".to_string(),
                "cursor".to_string(),
                "--model".to_string(),
                "auto".to_string(),
                "--key".to_string(),
                "alt+y".to_string(),
                "--default-cast".to_string(),
                "/home/user/cast.toml".to_string(),
                "--as".to_string(),
                "user".to_string(),
            ]
        );
    }

    let reply = app.action_rx.recv().await.expect("hw_setup reply");
    app.on_action_reply(reply);
    let updated = app.hollywood_cfg.as_ref().expect("hollywood cfg updated");
    assert_eq!(updated.runner, "cursor");
    assert_eq!(updated.model, "auto");
    assert_eq!(updated.effort, None);
    assert_eq!(updated.key.as_deref(), Some("alt+y"));
    assert_eq!(
        updated.default_cast.as_deref(),
        Some("/home/user/cast.toml")
    );
}

/// The selection can outlive its row — a publish drops the studio the
/// cursor sat on. `m` must not silently target the title (index 0 has
/// no menu, so the press looked dead in the sim): the fallback is the
/// first frame, and its menu opens.
#[tokio::test]
async fn a_vanished_selection_falls_back_to_the_first_frame_for_m() {
    let mut app = app_with_hollywood();
    app.area = (120, 30);
    alt_y(&mut app);
    draw_to(&mut app, 120, 30);
    // A studio that is no longer in the document — as if a publish
    // dropped it between the draw and the keypress.
    app.hollywood_select(Target::Studio(crate::studio::StudioKey {
        machine: None,
        id: "gone-shop".into(),
    }));
    key(&mut app, hc_vt::Key::Char('m'));
    let Some(menu) = app.menu.as_ref() else {
        panic!("the fallback frame's menu opens, not silence");
    };
    let ContextMenuTarget::Studio(menu) = &menu.target else {
        panic!("a frame's menu is the studio's");
    };
    assert_eq!(
        menu.studio.as_ref().map(|key| key.id.as_str()),
        Some("acme-shop"),
        "the first frame, not the title"
    );
}

#[tokio::test]
async fn m_on_a_talk_or_headline_row_offers_raw_hollywood_terminal_focus() {
    let mut app = app_with_hollywood();
    let mut doc = summary_json();
    doc["hollywood"]["headline_seq"] = serde_json::json!(9);
    doc["hollywood"]["exchange"] = serde_json::json!([
        { "seq": 8, "who": "user", "text": "status?" },
        { "seq": 9, "who": "hollywood", "text": "acme-shop needs a look" }
    ]);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.area = (120, 30);
    alt_y(&mut app);
    draw_to(&mut app, 120, 30);
    app.hollywood_select(Target::Talk(9));
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(app.menu.is_some(), "`m` on Talk opens the entry menu");
    let text = screen_lines(&draw_to(&mut app, 120, 30)).join("\n");
    assert!(text.contains("Read entry #9"), "{text}");
    assert!(text.contains("Focus Hollywood terminal"), "{text}");

    key(&mut app, hc_vt::Key::Char('t'));
    assert_eq!(app.focused.as_deref(), Some("t10"));
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
}

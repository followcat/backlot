//! Round 8 Slice S1 tests: Alt+Y into Compose, Call Sheet `›` history,
//! Esc resting, 3-zone Tab/Shift+Tab ring, adaptive Compose hints, and
//! non-collision with swapped Alt+arrows (`acme-shop`, `beta-tools`).
use super::*;
use crate::hollywood::Target;
use crate::panel::{PanelMode, Target as PanelTarget};

fn type_str(app: &mut App, s: &str) {
    for ch in s.chars() {
        key(app, hc_vt::Key::Char(ch));
    }
}

fn doc_with(value: serde_json::Value) -> App {
    let mut app = app_with_doc();
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.hollywood.mode = LayerMode::Collapsed;
    app.set_hollywood(Ok(Some(cfg(None))), std::path::Path::new("config.toml"));
    app
}

fn app_with_two_live_studios_and_talk() -> App {
    let mut value = summary_json();
    value["studios"][1]["state"] = serde_json::json!("live");
    value["studios"][1]["roles"] = serde_json::json!([
        { "role": "producer", "name": "producer", "terminal": "t2",
          "state": "live", "work": "working" }
    ]);
    value["hollywood"]["exchange"] = serde_json::json!([
        { "seq": 10, "who": "user", "text": "older status ask" },
        { "seq": 11, "who": "hollywood", "text": "all quiet on acme-shop" }
    ]);
    doc_with(value)
}

fn app_calm_with_exchange(with_talk: bool) -> App {
    let mut value = summary_json();
    value["hollywood"]["cards"] = serde_json::json!([]);
    if with_talk {
        value["hollywood"]["exchange"] = serde_json::json!([
            { "seq": 10, "who": "user", "text": "first msg" },
            { "seq": 11, "who": "hollywood", "text": "latest reply" }
        ]);
    }
    doc_with(value)
}

#[tokio::test]
async fn alt_y_from_hidden_or_collapsed_opens_compose_even_with_waits() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    app.hollywood.mode = LayerMode::Hidden;
    app.hollywood.selected = None;
    app.hollywood.editing = false;

    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert!(app.hollywood.editing, "Alt+Y from Hidden enters Compose");
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(
        text.contains("pause t1"),
        "waiting card stays visible: {text}"
    );

    type_str(&mut app, "keep this draft");
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    assert!(!app.hollywood.editing);
    assert_eq!(app.hollywood.input.as_str(), "keep this draft");

    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert!(app.hollywood.editing, "Alt+Y from Collapsed enters Compose");
    assert_eq!(app.hollywood.input.as_str(), "keep this draft");

    // Unconfigured `[hollywood]` keeps wait/frame landing in Resting.
    let mut uncfg = app_with_doc();
    uncfg.area = (120, 24);
    uncfg.hollywood.mode = LayerMode::Hidden;
    uncfg.set_hollywood(Ok(None), std::path::Path::new("config.toml"));
    alt_y(&mut uncfg);
    assert_eq!(uncfg.hollywood.mode, LayerMode::Expanded);
    assert!(!uncfg.hollywood.editing);
    assert_ne!(uncfg.hollywood.selected, Some(Target::Input));

    // Toast jump (`prefix+o` / notification focus) still lands on its item in Resting.
    let mut toast_app = app_with_hollywood();
    toast_app.area = (120, 24);
    toast_app.notifications.visible = Some(crate::app::notifications::Notification {
        event: crate::app::notifications::NotificationEvent {
            kind: crate::app::notifications::NotificationKind::NeedsAttention,
            title: "card waiting".into(),
            body: None,
            sound: None,
            agent: None,
            endpoint_id: crate::chrome::snapshot::ClientEndpointId::Local,
            workspace_id: None,
            tab_id: None,
            pane_id: None,
            studio: Some(Target::Card("c3".into())),
        },
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(60),
    });
    toast_app.focus_visible_notification();
    assert_eq!(toast_app.hollywood.mode, LayerMode::Expanded);
    assert_eq!(
        toast_app.hollywood.selected,
        Some(Target::Card("c3".into()))
    );
    assert!(!toast_app.hollywood.editing, "toast jump lands in Resting");
}

#[tokio::test]
async fn compose_up_down_recalls_history_and_restores_draft() {
    let mut empty_app = app_with_hollywood();
    empty_app.area = (120, 24);
    alt_y(&mut empty_app);
    assert!(empty_app.hollywood.editing);
    type_str(&mut empty_app, "partial");
    key(&mut empty_app, hc_vt::Key::ArrowUp);
    assert!(empty_app.hollywood.editing);
    assert_eq!(empty_app.hollywood.selected, Some(Target::Input));
    assert_eq!(empty_app.hollywood.input.as_str(), "partial");
    key(&mut empty_app, hc_vt::Key::ArrowDown);
    assert!(empty_app.hollywood.editing);
    assert_eq!(empty_app.hollywood.input.as_str(), "partial");

    let mut value = summary_json();
    value["hollywood"]["exchange"] = serde_json::json!([
        { "seq": 1, "who": "user", "text": "first seeded" },
        { "seq": 2, "who": "user", "text": "first seeded" },
        { "seq": 3, "who": "hollywood", "text": "hollywood reply" },
        { "seq": 4, "who": "user", "text": "second seeded" }
    ]);
    let mut app = doc_with(value);
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);

    alt_y(&mut app);
    assert!(app.hollywood.editing);
    type_str(&mut app, "first ask");
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.hollywood.editing, "Call Sheet stays in Compose on send");
    type_str(&mut app, "second ask");
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[
            vec!["tell", "hollywood", "first ask", "--as", "user"],
            vec!["tell", "hollywood", "second ask", "--as", "user"],
        ]
    );

    type_str(&mut app, "wip draft");
    key(&mut app, hc_vt::Key::ArrowUp);
    assert!(app.hollywood.editing, "Up never leaves Compose");
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert_eq!(app.hollywood.input.as_str(), "second ask");
    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(app.hollywood.input.as_str(), "first ask");
    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(app.hollywood.input.as_str(), "second seeded");
    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(app.hollywood.input.as_str(), "first seeded");

    // Editing recalled entry never mutates history; Down restores draft.
    key(&mut app, hc_vt::Key::Char('!'));
    assert_eq!(app.hollywood.input.as_str(), "first seeded!");
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.hollywood.input.as_str(), "second seeded");
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.hollywood.input.as_str(), "first ask");
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.hollywood.input.as_str(), "second ask");
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.hollywood.input.as_str(), "wip draft");

    key(&mut app, hc_vt::Key::ArrowUp);
    type_str(&mut app, " v2");
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().last().unwrap().as_slice(),
        &["tell", "hollywood", "second ask v2", "--as", "user"]
    );
    assert!(app.hollywood.editing);

    // Summary catching up must not duplicate session sends.
    let mut synced = summary_json();
    synced["hollywood"]["exchange"] = serde_json::json!([
        { "seq": 1, "who": "user", "text": "first seeded" },
        { "seq": 2, "who": "user", "text": "first seeded" },
        { "seq": 3, "who": "hollywood", "text": "hollywood reply" },
        { "seq": 4, "who": "user", "text": "second seeded" },
        { "seq": 5, "who": "user", "text": "first ask" },
        { "seq": 6, "who": "user", "text": "second ask" },
        { "seq": 7, "who": "user", "text": "second ask v2" }
    ]);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: synced,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    for expected in [
        "second ask v2",
        "second ask",
        "first ask",
        "second seeded",
        "first seeded",
    ] {
        key(&mut app, hc_vt::Key::ArrowUp);
        assert_eq!(app.hollywood.input.as_str(), expected);
    }
}

#[tokio::test]
async fn compose_esc_rests_and_tab_hits_first_wait() {
    let mut app = app_with_two_live_studios_and_talk();
    app.area = (120, 24);
    alt_y(&mut app);
    assert!(app.hollywood.editing);
    assert_eq!(app.hollywood.selected, Some(Target::Input));

    // Screen order: `Hollywood (Input) -> Studios -> Waiting (c3) -> Hollywood`,
    // keeping draft across zone switches.
    type_str(&mut app, "keep draft");
    key(&mut app, hc_vt::Key::Tab);
    assert!(!app.hollywood.editing);
    assert!(matches!(app.hollywood.selected, Some(Target::Studio(_))));
    assert_eq!(app.hollywood.input.as_str(), "keep draft");

    key(&mut app, hc_vt::Key::Tab);
    assert!(!app.hollywood.editing);
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));

    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert!(app.hollywood.editing);
    assert_eq!(app.hollywood.input.as_str(), "keep draft");

    key_mods(&mut app, hc_vt::Key::BackTab, hc_vt::Mods::SHIFT);
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
    key_mods(&mut app, hc_vt::Key::BackTab, hc_vt::Mods::SHIFT);
    assert!(matches!(app.hollywood.selected, Some(Target::Studio(_))));
    key_mods(&mut app, hc_vt::Key::BackTab, hc_vt::Mods::SHIFT);
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert!(app.hollywood.editing);

    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    key_mods(&mut app, hc_vt::Key::BackTab, hc_vt::Mods::SHIFT);
    key(&mut app, hc_vt::Key::Char('a'));
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec!["card", "apply", "c3", "--as", "user"]]
    );

    // Tab skips empty Waiting (`Talk` alone sits in Hollywood): `Compose -> Studios -> Compose`.
    let mut talk_app = app_calm_with_exchange(true);
    talk_app.area = (120, 24);
    alt_y(&mut talk_app);
    key(&mut talk_app, hc_vt::Key::Tab);
    assert!(matches!(
        talk_app.hollywood.selected,
        Some(Target::Studio(_))
    ));
    key(&mut talk_app, hc_vt::Key::Tab);
    assert!(talk_app.hollywood.editing);
    assert_eq!(talk_app.hollywood.selected, Some(Target::Input));
    // `Shift+Tab` while Waiting is empty returns to Studios without visiting Talk.
    key(&mut talk_app, hc_vt::Key::BackTab);
    assert!(matches!(
        talk_app.hollywood.selected,
        Some(Target::Studio(_))
    ));

    let mut bare_app = app_calm_with_exchange(false);
    bare_app.area = (120, 24);
    alt_y(&mut bare_app);
    key(&mut bare_app, hc_vt::Key::Tab);
    assert!(matches!(
        bare_app.hollywood.selected,
        Some(Target::Studio(_))
    ));
    key(&mut bare_app, hc_vt::Key::Tab);
    assert!(bare_app.hollywood.editing);

    // Esc on empty Compose rests on Target::Input (never collapses); Up hits newest Talk.
    let mut esc_app = app_calm_with_exchange(true);
    esc_app.area = (120, 24);
    alt_y(&mut esc_app);
    key(&mut esc_app, hc_vt::Key::Escape);
    assert_eq!(esc_app.hollywood.mode, LayerMode::Expanded);
    assert_eq!(esc_app.hollywood.selected, Some(Target::Input));
    assert!(!esc_app.hollywood.editing);
    key(&mut esc_app, hc_vt::Key::ArrowUp);
    assert_eq!(esc_app.hollywood.selected, Some(Target::Talk(11)));
    key(&mut esc_app, hc_vt::Key::Escape);
    assert_eq!(esc_app.hollywood.mode, LayerMode::Collapsed);
}

#[tokio::test]
async fn no_alt_arrow_collision_in_compose() {
    let mut app = app_with_two_live_studios_and_talk();
    app.area = (140, 30);
    app.focused = Some("t1".into());
    app.reconcile_published_view();

    // 1. Call Sheet Compose: Alt+Down switches role; draft preserved.
    alt_y(&mut app);
    assert!(app.hollywood.editing);
    type_str(&mut app, "my draft");
    key_mods(&mut app, hc_vt::Key::ArrowDown, hc_vt::Mods::ALT);
    assert_eq!(app.focused.as_deref(), Some("t4"), "Alt+Down cycles role");
    assert!(!app.hollywood.editing);
    assert_eq!(app.hollywood.input.as_str(), "my draft");

    // Re-enter Call Sheet Compose: Alt+PgDn switches studio; Alt+Down focuses role.
    alt_y(&mut app);
    assert!(app.hollywood.editing);
    key_mods(&mut app, hc_vt::Key::PageDown, hc_vt::Mods::ALT);
    assert_eq!(
        app.studio.current.as_ref().map(|k| k.id.as_str()),
        Some("beta-tools"),
        "Alt+PgDn switches studio"
    );
    assert!(!app.hollywood.expanded());
    key_mods(&mut app, hc_vt::Key::ArrowDown, hc_vt::Mods::ALT);
    assert_eq!(app.focused.as_deref(), Some("t2"));

    // 2. Studio Dock Compose: Alt+PgUp switches studio back, never eaten by history.
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(app.panel.mode, PanelMode::Expanded);
    key(&mut app, hc_vt::Key::Char('i'));
    assert!(app.panel_editing());
    assert!(matches!(app.panel.selected, Some(PanelTarget::Input(_))));
    key_mods(&mut app, hc_vt::Key::PageUp, hc_vt::Mods::ALT);
    assert_eq!(
        app.studio.current.as_ref().map(|k| k.id.as_str()),
        Some("acme-shop"),
        "Alt+PgUp switches studio out of Dock Compose"
    );
    assert!(!app.panel_editing());
}

#[tokio::test]
async fn compose_footer_hints_adapt_with_waits_and_width() {
    let mut with_waits = app_with_hollywood();
    with_waits.area = (140, 24);
    alt_y(&mut with_waits);
    let wide = screen_lines(&draw_to(&mut with_waits, 140, 24)).join("\n");
    assert!(wide.contains("⏎ send  ↑↓ history  tab next zone  esc rows  alt+y close"));
    let mid = screen_lines(&draw_to(&mut with_waits, 52, 24)).join("\n");
    assert!(mid.contains("⏎ send  ↑↓ hist  tab zone  esc rows"));
    let tight = screen_lines(&draw_to(&mut with_waits, 30, 24)).join("\n");
    assert!(tight.contains("⏎ ↑↓ esc"));

    let mut no_waits = app_calm_with_exchange(false);
    no_waits.area = (140, 24);
    alt_y(&mut no_waits);
    let wide = screen_lines(&draw_to(&mut no_waits, 140, 24)).join("\n");
    assert!(wide.contains("⏎ send  ↑↓ history  tab next zone  esc rows  alt+y close"));
    let mid = screen_lines(&draw_to(&mut no_waits, 52, 24)).join("\n");
    assert!(mid.contains("⏎ send  ↑↓ hist  tab zone  esc rows"));
    let tight = screen_lines(&draw_to(&mut no_waits, 30, 24)).join("\n");
    assert!(tight.contains("⏎ ↑↓ esc"));
}

//! The `›` line's cross-cutting `Compose` behavior: R3-B3's
//! per-studio drafts (one studio's text never paints on another's
//! row), R3-B2's paste/IME routing (a resting line and the focused
//! dock eat `Paste`/`Text` so nothing leaks to the pane), R3-M11's
//! Home/End (rows while `Resting`, the editor cursor while
//! composing) and R3-N7's finished-studio `↺ reopen producer`
//! row (`o` reopens, `⏎` only names the key).
use super::*;

/// R3-B3: drafts are per-studio — typing on `acme-shop`'s `›` never
/// paints into `beta-tools · host-b`'s line, and each keeps its own
/// parked text across rests.
#[tokio::test]
async fn each_studio_keeps_its_own_draft() {
    let mut app = app_with_two_machines();
    app.area = (140, 30);
    open_panel(&mut app);
    let beta = crate::studio::StudioKey {
        machine: Some("host-b".into()),
        id: "beta-tools".into(),
    };
    // Compose on acme-shop, park a draft, rest.
    key(&mut app, hc_vt::Key::Char('i'));
    for ch in "local line".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(draft(&app, "acme-shop"), "local line");
    // Land on beta-tools' `›` row and arm its Compose.
    app.panel.selected = Some(Target::Input(beta.clone()));
    key(&mut app, hc_vt::Key::Char('i'));
    assert_eq!(app.panel.selected, Some(Target::Input(beta.clone())));
    assert!(app.panel_editing());
    assert_eq!(
        app.panel
            .draft(&beta)
            .map(|d| d.as_str().to_owned())
            .as_deref(),
        Some(""),
        "beta-tools' own draft starts empty"
    );
    for ch in "remote line".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    // Each studio's draft answers for itself — acme-shop's parked
    // text never leaked into beta-tools' editor or row.
    assert_eq!(draft(&app, "acme-shop"), "local line");
    assert_eq!(
        app.panel
            .draft(&beta)
            .map(|d| d.as_str().to_owned())
            .as_deref(),
        Some("remote line")
    );
}

/// R3-B2's paste/IME routing: a composing `›` takes `Paste`/`Text`
/// as editor input; a resting one — and the focused dock in
/// general — eats them so nothing leaks to the terminal behind it.
#[tokio::test]
async fn paste_and_ime_land_only_in_a_composing_draft() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('i'));
    app.on_input(UiEvent::Paste(b"via paste".to_vec()), 120);
    assert_eq!(draft(&app, "acme-shop"), "via paste");
    app.on_input(UiEvent::Text(" + ime".into()), 120);
    assert_eq!(draft(&app, "acme-shop"), "via paste + ime");
    // Resting: the same events are eaten — the parked draft and the
    // pane's input queue are both untouched.
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.panel_editing());
    app.on_input(UiEvent::Paste(b"rest".to_vec()), 120);
    app.on_input(UiEvent::Text("text".into()), 120);
    assert_eq!(draft(&app, "acme-shop"), "via paste + ime");
    assert_eq!(app.pane.input_backlog().0, 0, "nothing leaked");
}

/// R3-M11's Home/End: `Resting` they are the body's first/last
/// actionable row; `Compose` hands them to the editor's cursor.
#[tokio::test]
async fn home_end_move_the_row_or_the_drafts_cursor() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    let first = app.panel.selected.clone();
    assert_eq!(first, Some(Target::Studio("acme-shop".into())));
    // Resting End lands on the body's last actionable row, Home
    // returns to the first — neither arms Compose on a `›`.
    key(&mut app, hc_vt::Key::End);
    let last = app.panel.selected.clone();
    assert!(last.is_some() && last != first, "End moved: {last:?}");
    assert!(!app.panel.editing);
    key(&mut app, hc_vt::Key::Home);
    assert_eq!(app.panel.selected, first);
    // Compose: Home/End are the editor's own — they move the cursor,
    // not the row.
    key(&mut app, hc_vt::Key::Char('i'));
    for ch in "abc".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Home);
    key(&mut app, hc_vt::Key::Char('x'));
    key(&mut app, hc_vt::Key::End);
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(draft(&app, "acme-shop"), "xabcy");
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
}

/// R3-N7: a finished studio closes its block with `↺ reopen
/// producer [o]` instead of a `›` line — `o` runs the reopen,
/// Enter only names the key.
#[tokio::test]
async fn a_finished_studio_offers_reopen_producer() {
    let mut app = app_with_panel_doc();
    let (recorded, run) = runner();
    app.set_panel_runner(run);
    app.area = (120, 24);
    // Finish acme-shop in place — `ready_to_finish` stays unset, so
    // the block reads the finished rows; its producer's seat is
    // closed like a real finish leaves it.
    let mut value = summary_json();
    value["studios"][0]["state"] = serde_json::json!("finished");
    value["studios"][0]["roles"][0]["state"] = serde_json::json!("closed");
    value["studios"][0]["roles"][0]["resumable"] = serde_json::json!(true);
    app.view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        Document {
            value,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    open_panel(&mut app);
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains("↺ reopen producer"), "{text}");
    assert!(
        app.panel_hits
            .rows
            .iter()
            .any(|(_, t)| matches!(t, Target::ReopenProducer(k) if k.id == "acme-shop")),
        "the reopen row is a target"
    );
    assert!(
        !app.panel_hits
            .rows
            .iter()
            .any(|(_, t)| matches!(t, Target::Input(k) if k.id == "acme-shop")),
        "a finished studio has no `›` line"
    );
    // `o` on the row reopens the fixed producer seat.
    app.panel.selected = Some(Target::ReopenProducer("acme-shop".into()));
    key(&mut app, hc_vt::Key::Char('o'));
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec![
            "reopen".to_string(),
            "--person".into(),
            "producer".into(),
            "--studio".into(),
            "acme-shop".into(),
            "--as".into(),
            "user".into(),
        ]]
    );
    // Enter only names the key — it never respawns.
    recorded.lock().unwrap().clear();
    key(&mut app, hc_vt::Key::Enter);
    assert!(recorded.lock().unwrap().is_empty());
    assert!(app.notice.is_some(), "the row explains `o`");
}

/// Round 8 S1: Studio Dock `›` recalls seeded `StudioSummary.exchange`
/// user entries plus session submits (tell & `/verb` except bare `/`),
/// stashing and restoring live drafts, and never leaves Compose on `↑`/`↓`.
#[tokio::test]
async fn compose_up_down_recalls_history_and_restores_draft() {
    // 1. Empty history: Up/Down keep draft and stay in Compose.
    let mut empty_app = app_with_panel_doc();
    empty_app.area = (120, 24);
    open_panel(&mut empty_app);
    key(&mut empty_app, hc_vt::Key::Char('i'));
    for ch in "wip".chars() {
        key(&mut empty_app, hc_vt::Key::Char(ch));
    }
    key(&mut empty_app, hc_vt::Key::ArrowUp);
    assert!(empty_app.panel_editing());
    assert_eq!(
        empty_app.panel.selected,
        Some(Target::Input("acme-shop".into()))
    );
    assert_eq!(draft(&empty_app, "acme-shop"), "wip");
    key(&mut empty_app, hc_vt::Key::ArrowDown);
    assert!(empty_app.panel_editing());
    assert_eq!(draft(&empty_app, "acme-shop"), "wip");

    // 2. Seeded exchange (`who == "user"`, skipping producer replies and collapsing repeats)
    //    plus session sends before next summary arrives.
    let mut app = app_with_panel_doc();
    let (recorded, run) = runner();
    app.set_panel_runner(run);
    app.area = (120, 24);
    let mut doc = summary_json();
    doc["studios"][0]["exchange"] = serde_json::json!([
        { "seq": 1, "who": "user", "text": "seeded one" },
        { "seq": 2, "who": "user", "text": "seeded one" },
        { "seq": 3, "who": "producer", "text": "working on it" },
        { "seq": 4, "who": "user", "text": "seeded two" }
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
    open_panel(&mut app);

    // Send 2 local lines before summary catches up (a plain tell and a /verb).
    key(&mut app, hc_vt::Key::Char('i'));
    for ch in "dock first".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    assert!(!app.panel_editing(), "Dock rests on submit");

    key(&mut app, hc_vt::Key::Char('i'));
    for ch in "/tell actor-alice check build".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    assert!(!app.panel_editing());
    assert_eq!(recorded.lock().unwrap().len(), 2);

    // Re-enter Compose, type partial draft, walk Up/Down.
    key(&mut app, hc_vt::Key::Char('i'));
    for ch in "draft".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::ArrowUp);
    assert!(app.panel_editing(), "Up stays in Dock Compose");
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    assert_eq!(draft(&app, "acme-shop"), "/tell actor-alice check build");

    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(draft(&app, "acme-shop"), "dock first");
    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(draft(&app, "acme-shop"), "seeded two");
    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(draft(&app, "acme-shop"), "seeded one");

    // Editing recalled entry does not mutate history; Down restores draft.
    key(&mut app, hc_vt::Key::Char('!'));
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(draft(&app, "acme-shop"), "seeded two");
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(draft(&app, "acme-shop"), "dock first");
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(draft(&app, "acme-shop"), "/tell actor-alice check build");
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(draft(&app, "acme-shop"), "draft");

    // Recall "dock first", edit and send as a new tell.
    key(&mut app, hc_vt::Key::ArrowUp);
    key(&mut app, hc_vt::Key::ArrowUp);
    for ch in " v2".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().last().unwrap().as_slice(),
        &[
            "tell",
            "producer",
            "--text",
            "dock first v2",
            "--studio",
            "acme-shop",
            "--as",
            "user",
        ]
    );
    assert!(!app.panel_editing());
    assert_eq!(draft(&app, "acme-shop"), "");

    // Summary catching up for `/tell ... check build` and `dock first` avoids duplicates.
    let mut synced = summary_json();
    synced["studios"][0]["exchange"] = serde_json::json!([
        { "seq": 1, "who": "user", "text": "seeded one" },
        { "seq": 2, "who": "user", "text": "seeded one" },
        { "seq": 3, "who": "producer", "text": "working on it" },
        { "seq": 4, "who": "user", "text": "seeded two" },
        { "seq": 5, "who": "user", "text": "dock first" },
        { "seq": 6, "who": "user", "text": "check build" },
        { "seq": 7, "who": "user", "text": "dock first v2" }
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
    key(&mut app, hc_vt::Key::Char('i'));
    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(draft(&app, "acme-shop"), "dock first v2");
    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(draft(&app, "acme-shop"), "/tell actor-alice check build");
    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(draft(&app, "acme-shop"), "dock first");

    // Esc on empty Dock Compose rests without unfocusing; second Esc unfocuses.
    let mut esc_app = app_with_panel_doc();
    esc_app.area = (120, 24);
    open_panel(&mut esc_app);
    key(&mut esc_app, hc_vt::Key::Char('i'));
    assert!(esc_app.panel_editing());
    key(&mut esc_app, hc_vt::Key::Escape);
    assert!(!esc_app.panel_editing());
    assert!(esc_app.panel.focused);
    assert_eq!(
        esc_app.panel.selected,
        Some(Target::Input("acme-shop".into()))
    );
    key(&mut esc_app, hc_vt::Key::Escape);
    assert!(!esc_app.panel.focused);
}

#[tokio::test]
async fn dock_compose_footer_hints_show_history() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('i'));
    let wide = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(
        wide.contains("⏎ send  ↑↓ history  esc done"),
        "wide: {wide}"
    );
    let mid = screen_lines(&draw_to(&mut app, 28, 24)).join("\n");
    assert!(mid.contains("⏎ ↑↓  esc"), "mid: {mid}");
    let tight = screen_lines(&draw_to(&mut app, 9, 24)).join("\n");
    assert!(tight.contains("esc"), "tight: {tight}");
}

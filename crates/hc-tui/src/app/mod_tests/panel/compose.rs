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

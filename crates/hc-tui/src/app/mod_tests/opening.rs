//! U2 opening page: the visibility latch, the drawn rows and the
//! keys it owns — through the real publish and routing paths.
//! Neutral fixtures only (`acme-shop`, `host-a`).
use super::*;
use crate::app::input::UiEvent;
use hc_proto::watch::{DocKey, Document};

fn key(app: &mut App, k: hc_vt::Key) {
    app.on_input(
        UiEvent::Key(term_key(k, hc_vt::Mods::NONE, hc_vt::Action::Press)),
        app.area.0,
    );
}

fn doc_key() -> DocKey {
    DocKey {
        machine: None,
        name: "backlot".into(),
    }
}

/// Publish a `backlot` document body.
fn publish_summary(app: &mut App, value: serde_json::Value) {
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
}

fn zero_studios() -> serde_json::Value {
    serde_json::json!({
        "schema": "backlot/1", "updated_ms": 2, "studios": []
    })
}

fn one_studio() -> serde_json::Value {
    serde_json::json!({
        "schema": "backlot/1", "updated_ms": 2,
        "studios": [{
            "id": "acme-shop", "title": "Acme shop", "state": "live",
            "requirement_version": 1, "mode": "standard",
            "open_reviews": 0, "waiting": 0,
            "roles": [{ "role": "producer", "name": "producer",
                        "terminal": "t1", "state": "live" }]
        }]
    })
}

/// A focused plain terminal, no `backlot` document — the start
/// state of a first run.
fn no_doc_app() -> App {
    let mut app = app_with(&["t1"]);
    app.set_plain(false);
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    app
}

// ---------- the latch ----------

#[tokio::test]
async fn no_document_shows_the_page() {
    let mut app = no_doc_app();
    assert!(app.opening_visible());
    let backend = draw_to(&mut app, 80, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("a film studio for coding agents"), "{text}");
    assert!(text.contains("New studio"), "{text}");
    assert!(text.contains("backlot --plain"), "{text}");
}

#[tokio::test]
async fn a_good_document_with_zero_studios_shows_the_page() {
    let mut app = no_doc_app();
    publish_summary(&mut app, zero_studios());
    // U2: studio mode is on whenever `--plain` was not given — the
    // page shows because no studio exists, not because the mode left.
    assert!(app.studio.active);
    assert!(app.opening_visible());
    let backend = draw_to(&mut app, 80, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("New studio"), "{text}");
}

#[tokio::test]
async fn a_studio_hides_the_page() {
    let mut app = no_doc_app();
    publish_summary(&mut app, one_studio());
    assert!(app.studio.active);
    assert!(!app.opening_visible());
    let backend = draw_to(&mut app, 80, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("cast · acme-shop"), "{text}");
}

#[tokio::test]
async fn plain_never_shows_the_page() {
    let mut app = no_doc_app();
    app.set_plain(true);
    assert!(!app.opening_visible());
    let backend = draw_to(&mut app, 80, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(!text.contains("New studio"), "{text}");
    assert!(text.contains("agents"), "{text}");
}

#[tokio::test]
async fn esc_hides_the_page_for_the_session() {
    let mut app = no_doc_app();
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.opening_visible());
    // The page is a cover, not a state — what's beneath shows.
    let backend = draw_to(&mut app, 80, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(!text.contains("New studio"), "{text}");
}

#[tokio::test]
async fn new_workspace_brings_the_page_back() {
    let mut app = no_doc_app();
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.opening_visible());
    app.on_action(crate::input::KeybindAction::NewWorkspace, 80);
    assert!(app.opening_visible());
    // Esc again dismisses it once more.
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.opening_visible());
}

#[tokio::test]
async fn new_workspace_over_a_studio_forces_the_page() {
    let mut app = no_doc_app();
    publish_summary(&mut app, one_studio());
    assert!(!app.opening_visible());
    app.on_action(crate::input::KeybindAction::NewWorkspace, 80);
    assert!(app.opening_visible(), "the new-studio flow covers the cast");
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.opening_visible());
}

#[tokio::test]
async fn plain_new_workspace_stays_herdrs() {
    let mut app = no_doc_app();
    app.set_plain(true);
    // `--plain`: the action is herdr's — no page, and the action
    // reaches the layout document path (a real workspace create
    // fails here only for lack of an operations wire).
    app.on_action(crate::input::KeybindAction::NewWorkspace, 80);
    assert!(!app.opening_visible());
    assert!(!app.opening.forced);
}

// ---------- the keys ----------

#[tokio::test]
async fn n_arms_the_wizard_popup() {
    let mut app = no_doc_app();
    key(&mut app, hc_vt::Key::Char('n'));
    let pending = app.popup_pending.as_ref().expect("the run is armed");
    assert!(!pending.persistent);
    // The action went to the operations queue — or, without a wire
    // in tests, to a notice; the pending geometry is what matters.
}

#[tokio::test]
async fn s_runs_stage_start_through_the_panel_runner() {
    let mut app = no_doc_app();
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_panel_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(()) })
    }));
    key(&mut app, hc_vt::Key::Char('s'));
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec!["stage".to_string(), "start".to_string()]]
    );
}

#[tokio::test]
async fn s_is_absent_while_the_stage_is_up() {
    let mut app = no_doc_app();
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    let mut terms: Vec<_> = app.view.inventory.terminals().values().cloned().collect();
    terms.push(stage);
    app.view.inventory_mut().replace(2, terms);
    app.reconcile_published_view();
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_panel_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(()) })
    }));
    key(&mut app, hc_vt::Key::Char('s'));
    assert!(recorded.lock().unwrap().is_empty(), "no second stage");
    // And the row is not offered on the page.
    let backend = draw_to(&mut app, 80, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(!text.contains("Start the stage"), "{text}");
}

#[tokio::test]
async fn q_detaches() {
    let mut app = no_doc_app();
    key(&mut app, hc_vt::Key::Char('q'));
    assert!(app.quitting);
}

#[tokio::test]
async fn the_page_owns_keys_not_its_own() {
    let mut app = no_doc_app();
    let before = app.pane.input_backlog().0;
    // A letter the page does not own is eaten — it never reaches the
    // hidden terminal beneath.
    key(&mut app, hc_vt::Key::Char('x'));
    assert_eq!(app.pane.input_backlog().0, before);
    assert!(app.opening_visible());
}

#[tokio::test]
async fn h_opens_the_hollywood_input_only_when_configured() {
    let mut app = no_doc_app();
    // Without `[hollywood]` the key does nothing.
    key(&mut app, hc_vt::Key::Char('h'));
    assert!(!app.hollywood.expanded());
    // With it: the layer expands on its input line.
    app.set_hollywood(
        Ok(Some(backlot_stage::config::HollywoodCfg {
            runner: "fake".into(),
            model: "m".into(),
            effort: None,
            key: None,
            default_cast: None,
        })),
        std::path::Path::new("/tmp/x"),
    );
    key(&mut app, hc_vt::Key::Char('h'));
    assert!(app.hollywood.expanded());
    assert!(app.hollywood.editing);
}

// ---------- drawn variants ----------

#[tokio::test]
async fn the_page_draws_at_200_80_and_40() {
    let mut app = no_doc_app();
    for width in [200u16, 80, 40] {
        let backend = draw_to(&mut app, width, 24);
        let text = screen_lines(&backend).join("\n");
        assert!(text.contains("New studio"), "width {width}:\n{text}");
        assert!(text.contains("detach"), "width {width}:\n{text}");
    }
}

#[tokio::test]
async fn the_h_row_needs_the_hollywood_config() {
    let mut app = no_doc_app();
    let backend = draw_to(&mut app, 80, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(!text.contains("Tell Hollywood"), "{text}");
}

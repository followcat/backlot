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

/// Write the `backlot` document slot with `value`, `fetching` as the
/// watcher would while the read is still on the wire.
fn put_document(app: &mut App, value: serde_json::Value, fetching: bool) {
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value,
            fetching,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
}

/// A focused terminal on a core whose first read landed with an
/// unwritten document — the start state of a first run, and the card.
fn no_doc_app() -> App {
    let mut app = app_with(&["t1"]);
    app.set_plain(false);
    app.focused = Some("t1".into());
    put_document(&mut app, serde_json::Value::Null, false);
    app
}

/// A core that has not answered the first read at all: no document
/// slot, so nothing is known yet.
fn blank_app() -> App {
    let mut app = app_with(&["t1"]);
    app.set_plain(false);
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    app
}

/// The same, with the read still on the wire carrying `value`.
fn reading_app(value: serde_json::Value) -> App {
    let mut app = app_with(&["t1", "t2"]);
    app.set_plain(false);
    app.focused = Some("t1".into());
    put_document(&mut app, value, true);
    app
}

// ---------- the entry wait (U7) ----------

#[tokio::test]
async fn nothing_draws_while_the_first_read_is_on_the_wire() {
    // The read carries an unwritten document: it is the gate alone
    // that keeps the card back.
    let mut app = reading_app(serde_json::Value::Null);
    assert!(!app.opening.ready(), "the read has not landed");
    assert!(!app.opening_visible());
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(!text.contains("New studio"), "{text}");
    // The one-time run never started, so it never flashes.
    assert!(app.opening_anim_deadline.is_none(), "the run armed");
    assert!(!app.opening.anim.done, "the run was left before it played");
    // The read lands with no studios: the card shows and the run
    // starts from its first frame.
    put_document(&mut app, serde_json::Value::Null, false);
    assert!(app.opening_visible());
    assert_eq!(app.opening_frame(), Some(0));
}

#[tokio::test]
async fn the_layer_waits_for_the_read_too() {
    let mut app = reading_app(one_studio());
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
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(!text.contains("hollywood"), "{text}");
    // The read lands: the layer is judged now.
    put_document(&mut app, one_studio(), false);
    assert!(app.opening.ready());
}

#[tokio::test]
async fn the_two_second_cap_shows_the_card() {
    use std::time::Duration;
    let mut app = blank_app();
    // The constructor's first sync armed the cap at the real now —
    // `get_or_insert` keeps that first arm, so a t0 taken a
    // millisecond late would sit inside the cap's last stretch and
    // the 1 ms-below-boundary pass would already settle. Re-arm at
    // the test's own t0: the exercise is the arithmetic, not the
    // constructor's clock.
    app.opening.cap = None;
    let t0 = std::time::Instant::now();
    app.settle_opening(t0);
    assert_eq!(
        app.opening.cap,
        Some(t0 + crate::opening::SETTLE_CAP),
        "the first pass arms the cap at its own now"
    );
    assert!(!app.opening_visible(), "the cap has not passed");
    app.settle_opening(t0 + Duration::from_millis(1_999));
    assert!(!app.opening_visible());
    app.settle_opening(t0 + crate::opening::SETTLE_CAP);
    assert!(app.opening_visible(), "judge with what is there");
    assert!(app.opening.cap.is_none(), "the wait left no timer");
}

#[tokio::test]
async fn a_settled_read_with_no_studios_shows_the_card() {
    let mut app = reading_app(serde_json::Value::Null);
    assert!(!app.opening_visible());
    put_document(&mut app, serde_json::Value::Null, false);
    assert!(app.opening_visible());
    let text = screen_lines(&draw_to(&mut app, 80, 24)).join("\n");
    assert!(text.contains("New studio"), "{text}");
}

#[tokio::test]
async fn a_settled_read_with_a_studio_never_shows_the_card() {
    let mut app = reading_app(one_studio());
    let text = screen_lines(&draw_to(&mut app, 80, 24)).join("\n");
    assert!(!text.contains("New studio"), "{text}");
    put_document(&mut app, one_studio(), false);
    assert!(!app.opening_visible());
    let text = screen_lines(&draw_to(&mut app, 80, 24)).join("\n");
    assert!(!text.contains("New studio"), "{text}");
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
    assert!(text.contains("acme-shop"), "{text}");
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
async fn new_workspace_over_a_studio_opens_the_dialog() {
    let mut app = no_doc_app();
    publish_summary(&mut app, one_studio());
    assert!(!app.opening_visible());
    app.on_action(crate::input::KeybindAction::NewWorkspace, 80);
    assert!(
        !app.opening_visible(),
        "the card must never come up over a studio"
    );
    assert!(
        app.new_studio.is_some(),
        "the native New Studio dialog opens over a studio"
    );
    assert!(app.popup_pending.is_none(), "no popup wizard any more");
}

#[tokio::test]
async fn new_workspace_without_a_studio_shows_the_card() {
    let mut app = no_doc_app();
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.opening_visible());
    app.on_action(crate::input::KeybindAction::NewWorkspace, 80);
    assert!(app.opening_visible());
    assert!(app.new_studio.is_none(), "the card owns the new flow");
}

#[tokio::test]
async fn plain_new_workspace_stays_herdrs() {
    let mut app = no_doc_app();
    app.set_plain(true);
    // `--plain`: the action is herdr's — no page, no dialog, and the
    // action reaches the layout document path (a real workspace
    // create fails here only for lack of an operations wire).
    app.on_action(crate::input::KeybindAction::NewWorkspace, 80);
    assert!(!app.opening_visible());
    assert!(app.new_studio.is_none(), "no dialog outside studio mode");
}

// ---------- the keys ----------

#[tokio::test]
async fn n_opens_the_new_studio_dialog() {
    let mut app = no_doc_app();
    key(&mut app, hc_vt::Key::Char('n'));
    assert!(app.new_studio.is_some(), "n opened the dialog");
    assert!(app.popup_pending.is_none(), "no popup is armed");
    // Esc on step 1 closes it — the Esc rule ends at closed.
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.new_studio.is_none());
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

// ---------- the one-time animation ----------

#[tokio::test]
async fn the_opening_plays_once_and_then_arms_nothing() {
    use std::time::Duration;
    let mut app = no_doc_app();
    let t0 = std::time::Instant::now();
    // The first draw starts the run at frame 0 and arms the next one.
    assert_eq!(app.opening_frame(), Some(0));
    app.arm_opening_anim(t0);
    assert_eq!(app.opening_anim_deadline, Some(t0 + crate::opening::FRAME));
    let mut now = t0;
    for _ in 1..crate::opening::OPENING_FRAMES {
        now += Duration::from_millis(80);
        app.tick_opening(now);
        let _ = app.opening_frame();
        app.arm_opening_anim(now);
    }
    assert!(app.opening.anim.done, "the run finished");
    assert!(app.opening_anim_deadline.is_none(), "idle arms nothing");
    assert_eq!(app.opening_frame(), None, "it never replays");
}

#[tokio::test]
async fn esc_before_the_last_frame_leaves_the_page_static() {
    let mut app = no_doc_app();
    let t0 = std::time::Instant::now();
    assert_eq!(app.opening_frame(), Some(0));
    app.arm_opening_anim(t0);
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.opening.anim.done, "leaving finishes the run");
    assert!(app.opening_anim_deadline.is_none());
    app.on_action(crate::input::KeybindAction::NewWorkspace, 80);
    assert!(app.opening_visible());
    assert_eq!(app.opening_frame(), None, "no replay after Esc");
}

// ---------- prefix chords over the opening page (R6) ----------

fn press_chord(app: &mut App, tail: hc_vt::Key) {
    for (k, mods) in [
        (hc_vt::Key::Char('b'), hc_vt::Mods::CTRL),
        (tail, hc_vt::Mods::NONE),
    ] {
        app.on_input(
            UiEvent::Key(term_key(k, mods, hc_vt::Action::Press)),
            app.area.0,
        );
    }
}

#[tokio::test]
async fn ctrl_b_t_focuses_the_left_tree_over_the_opening_page_and_esc_returns() {
    let mut app = no_doc_app();
    app.area = (120, 24);
    assert!(app.opening_visible());
    assert!(!app.tree_focused());

    press_chord(&mut app, hc_vt::Key::Char('t'));
    assert!(
        app.tree_focused(),
        "Ctrl+B t must focus the left tree while Opening Page is visible"
    );
    assert!(app.tree_focus.is_some());
    assert!(app.opening_visible());

    // Tree navigation works while Opening Page is underneath.
    let before_pane = app.pane.input_backlog().0;
    key(&mut app, hc_vt::Key::Char('j'));
    assert_eq!(app.pane.input_backlog().0, before_pane);
    assert!(app.tree_focused());

    // Esc leaves the tree cleanly without dismissing the Opening Page.
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.tree_focused(), "Esc leaves tree focus cleanly");
    assert!(app.opening_visible(), "Opening Page stays visible");
}

#[tokio::test]
async fn ctrl_b_question_opens_the_action_launcher_over_the_opening_page() {
    let mut app = no_doc_app();
    app.area = (120, 24);
    assert!(app.opening_visible());
    assert!(!app.help);

    press_chord(&mut app, hc_vt::Key::Char('?'));
    assert!(
        app.help,
        "Ctrl+B ? must open the action launcher while Opening Page is visible"
    );

    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.help);
    assert!(app.opening_visible());
}

#[tokio::test]
async fn ctrl_b_s_does_not_trigger_opening_stage_start() {
    let mut app = no_doc_app();
    app.area = (120, 24);
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_panel_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(()) })
    }));

    press_chord(&mut app, hc_vt::Key::Char('s'));
    assert!(
        recorded.lock().unwrap().is_empty(),
        "Ctrl+B s must not fire Opening Page's bare `s` stage start"
    );
}

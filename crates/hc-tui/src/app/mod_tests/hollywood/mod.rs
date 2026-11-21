//! The Hollywood layer's App wiring tests (H4): the `[hollywood]`
//! gate on `alt+y`, the three-state cycle, the no-resize guarantee
//! and the absent-table parity. Keys live in `keys`, actions in
//! `actions`, mouse in `mouse`. Neutral fixtures only (`acme-shop`,
//! `beta-tools`, `alice`, `host-a`).
mod actions;
mod cursor;
mod entry;
mod finish_key;
mod keys;
mod mouse;
mod opening;
mod setup;

use super::*;
use crate::app::input::{MouseButton, MouseEventKind, MouseInput, UiEvent};
use crate::hollywood::{LayerMode, Target};
use backlot_stage::config::HollywoodCfg;
use hc_proto::watch::{DocKey, Document};

fn cfg(key: Option<&str>) -> HollywoodCfg {
    HollywoodCfg {
        runner: "fake".into(),
        model: "model-h".into(),
        effort: Some("high".into()),
        key: key.map(str::to_string),
        default_cast: None,
    }
}

/// Two studios — acme-shop live with a waiting actor and a producer
/// on `t1`, beta-tools finished — under a live Hollywood with a
/// headline and two open cards: `c3` a pause, `c4` a studio-new.
fn summary_json() -> serde_json::Value {
    serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "studios": [
            {
                "id": "acme-shop", "title": "Acme shop", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 1,
                "next": "review delivery 1 — director",
                "roles": [
                    { "role": "producer", "name": "producer", "terminal": "t1",
                      "state": "live", "work": "working" },
                    { "role": "actor-alice", "name": "alice", "terminal": "t4",
                      "runner": "fake", "model": "m", "state": "live",
                      "work": "blocked", "task": "t1" }
                ]
            },
            {
                "id": "beta-tools", "title": "Beta tools", "state": "finished",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "roles": []
            }
        ],
        "hollywood": {
            "state": "live",
            "runner": "fake",
            "model": "model-h",
            "effort": "high",
            "headline": "acme-shop needs a look",
            "cards": [
                { "id": "c3", "studio": "acme-shop",
                  "action": "pause t1", "why": "the actor is looping" },
                { "id": "c4", "studio": "beta-tools",
                  "action": "studio-new beta-tools 'Beta tools'",
                  "why": "the user asked for a second shop" }
            ]
        }
    })
}

/// The app's `backlot` document carrying `summary_json`.
fn app_with_doc() -> App {
    let mut app = app_with(&["t1", "t2", "t3", "t4", "t5"]);
    app.set_plain(false);
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    let mut hwood = crate::sidebar::fixture::term("t10");
    hwood.name = Some("bl-hollywood".into());
    hwood.state = "live".into();
    let terminals: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .cloned()
        .chain([stage, hwood])
        .collect();
    app.view.inventory_mut().replace(2, terminals);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: summary_json(),
            ..Document::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    // The publish the loop would deliver: projection, pane targets and
    // U7's entry decision all run from it.
    app.reconcile_published_view();
    app
}

/// The document plus a configured layer, collapsed — the state a
/// client that entered without studios meets, and the one a toggle
/// leaves behind. A client with studios starts expanded (U7), so the
/// strip tests collapse it here.
fn app_with_hollywood() -> App {
    let mut app = app_with_doc();
    app.hollywood.mode = LayerMode::Collapsed;
    app.set_hollywood(Ok(Some(cfg(None))), std::path::Path::new("config.toml"));
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    app
}

fn doc_key() -> DocKey {
    DocKey {
        machine: None,
        name: "backlot".into(),
    }
}

fn key(app: &mut App, k: hc_vt::Key) {
    app.on_input(
        UiEvent::Key(term_key(k, hc_vt::Mods::NONE, hc_vt::Action::Press)),
        app.area.0,
    );
}

fn key_mods(app: &mut App, k: hc_vt::Key, mods: hc_vt::Mods) {
    app.on_input(
        UiEvent::Key(term_key(k, mods, hc_vt::Action::Press)),
        app.area.0,
    );
}

fn alt_y(app: &mut App) {
    key_mods(app, hc_vt::Key::Char('y'), hc_vt::Mods::ALT);
}

fn prefix(app: &mut App) {
    key_mods(app, hc_vt::Key::Char('b'), hc_vt::Mods::CTRL);
}

fn mouse(kind: MouseEventKind, col: u16, row: u16) -> MouseInput {
    MouseInput {
        kind,
        col,
        row,
        mods: hc_vt::Mods::NONE,
        sgr_position: None,
    }
}

fn recorder() -> (
    std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    crate::app::action_tracker::TestRunner,
) {
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    let runner: crate::app::action_tracker::TestRunner = std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(String::new()) })
    });
    (recorded, runner)
}

// ---------- parity (D6) ----------

// ---------- the no-agent bar (U7) ----------

#[tokio::test]
async fn without_the_table_the_bar_still_exists_without_an_agent() {
    let mut app = app_with_doc();
    app.area = (120, 24);
    // `Ok(None)` — the absent read the cli path hands the layer.
    app.set_hollywood(Ok(None), std::path::Path::new("config.toml"));
    assert!(
        app.hollywood.visible(),
        "studio mode always has the bar (U7)"
    );
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains('✦'), "the lot pill: {text}");
    // The toggle is the default one without a table.
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains("no agent"), "{text}");
    assert!(text.contains("hollywood setup"), "{text}");
    assert!(!text.contains("tell hollywood"), "no input line: {text}");
    assert!(
        !text.contains("acme-shop needs a look"),
        "no headline without an agent: {text}"
    );
}

#[tokio::test]
async fn plain_has_no_layer_and_alt_y_reaches_the_pane() {
    let mut app = app_with_doc();
    app.area = (120, 24);
    app.set_plain(true);
    app.set_hollywood(Ok(Some(cfg(None))), std::path::Path::new("config.toml"));
    assert_eq!(app.hollywood.mode, LayerMode::Hidden);
    let before = app.pane.input_backlog().0;
    alt_y(&mut app);
    assert!(app.pane.input_backlog().0 > before, "alt+y was consumed");
    assert_eq!(app.hollywood.mode, LayerMode::Hidden);
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(!text.contains("hollywood"), "{text}");
}

#[tokio::test]
async fn turning_plain_on_drops_the_layer_and_freeing_it_brings_it_back() {
    let mut app = app_with_doc();
    app.area = (120, 24);
    app.set_hollywood(Ok(None), std::path::Path::new("config.toml"));
    assert!(app.hollywood.visible());
    app.set_plain(true);
    assert_eq!(app.hollywood.mode, LayerMode::Hidden, "--plain hides it");
    let before = app.pane.input_backlog().0;
    alt_y(&mut app);
    assert!(app.pane.input_backlog().0 > before, "alt+y was consumed");
    app.set_plain(false);
    assert!(app.hollywood.visible(), "studio mode brings the bar back");
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
}

#[tokio::test]
async fn a_malformed_table_notices_and_leaves_the_no_agent_bar() {
    let mut app = app_with_doc();
    app.area = (120, 24);
    app.set_hollywood(
        Err("config.toml: malformed at line 2 column 1".into()),
        std::path::Path::new("config.toml"),
    );
    assert!(app.notice.is_some(), "the bad read notices once");
    assert!(!app.has_hollywood_cfg, "no agent is configured");
    // The bar survives a bad read in its no-agent form, on the
    // default toggle — collapsed, its face the lot pill.
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains('✦'), "the lot pill: {text}");
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains("no agent"), "{text}");
}

// ---------- geometry ----------

#[tokio::test]
async fn the_layer_never_resizes_a_pane_at_any_width() {
    let mut app = app_with_hollywood();
    for cols in [200u16, 120, 80, 40] {
        for mode in [LayerMode::Hidden, LayerMode::Collapsed, LayerMode::Expanded] {
            app.hollywood.mode = mode;
            app.area = (cols, 24);
            let direct = app.shell.layout_config().layout(
                cols,
                24,
                app.shell_state.sidebar_collapsed,
                crate::app::pane_geometry::focused_tab_count(&app),
                app.sidebar_width,
                app.studio.lot.pill || app.studio.lot.header.is_some(),
            );
            assert_eq!(
                app.chrome_layout(cols, 24),
                direct,
                "cols {cols} mode {mode:?}"
            );
        }
    }
}

// ---------- the entry state (U7) ----------

/// A settled document with no studios.
fn zero_studio_json() -> serde_json::Value {
    serde_json::json!({ "schema": "backlot/1", "updated_ms": 1, "studios": [] })
}

/// A client whose first read landed with no studio: the card is up.
fn app_with_empty_doc() -> App {
    let mut app = app_with(&["t1"]);
    app.set_plain(false);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: zero_studio_json(),
            ..Document::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    app
}

#[tokio::test]
async fn a_client_with_studios_starts_the_bar_collapsed() {
    let mut app = app_with_doc();
    assert_eq!(
        app.hollywood.mode,
        LayerMode::Collapsed,
        "the bar starts collapsed — the pill is its face (S0)"
    );
    // `alt+y` opens it, as before; Esc collapses it back.
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains("hollywood"), "{text}");
    assert!(text.contains("acme-shop"), "{text}");
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    // `Esc`/the toggle are the sheet's closes now — `x` is a card's
    // cancel (S1), not a hide.
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
}

#[tokio::test]
async fn a_client_without_studios_keeps_the_bar_off_the_card() {
    let mut app = app_with_empty_doc();
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    assert!(app.opening_visible());
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(!text.contains("hollywood"), "{text}");
    assert!(text.contains("New studio"), "{text}");
}

#[tokio::test]
async fn the_first_studio_leaves_the_bar_collapsed() {
    let mut app = app_with_empty_doc();
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: summary_json(),
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    assert!(!app.opening_visible());
    assert_eq!(
        app.hollywood.mode,
        LayerMode::Collapsed,
        "a studio that arrives later does not pull the bar open"
    );
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains('✦'), "the lot pill: {text}");
    assert!(text.contains('2'), "two studios in the pill: {text}");
}

// ---------- the toggle ----------

#[tokio::test]
async fn alt_y_cycles_strip_then_panel_then_strip() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    // From hidden the toggle shows the strip again.
    app.hollywood.mode = LayerMode::Hidden;
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    // Nothing reached the program through any of it.
    assert_eq!(app.pane.input_backlog().0, 0, "the toggle leaked");
}

#[tokio::test]
async fn the_lot_pill_sits_at_the_bar_s_right_end_and_opens_on_click() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    let backend = draw_to(&mut app, 120, 24);
    let top = &screen_lines(&backend)[0];
    assert!(top.contains('✦'), "the lot pill: {top}");
    assert!(top.contains("2 cards"), "the card count: {top}");
    let pill = app.hit_map.lot_pill;
    assert_eq!(pill.height, 1);
    assert_eq!(pill.x + pill.width, 120, "the right end of the row");
    // A click on it opens the bar, like `alt+y`.
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        pill.x,
        pill.y,
    ));
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
}

#[tokio::test]
async fn the_setup_hint_shows_in_the_panel_only_without_the_table() {
    // Without `[hollywood]` the expanded studio panel names the
    // setup command (R22); with it the row is gone.
    let mut app = app_with_doc();
    app.area = (200, 24);
    app.set_hollywood(Ok(None), std::path::Path::new("config.toml"));
    app.panel.mode = crate::panel::PanelMode::Expanded;
    let text = screen_lines(&draw_to(&mut app, 200, 24)).join("\n");
    assert!(text.contains("hollywood setup"), "{text}");
    app.set_hollywood(Ok(Some(cfg(None))), std::path::Path::new("config.toml"));
    let text = screen_lines(&draw_to(&mut app, 200, 24)).join("\n");
    assert!(!text.contains("hollywood setup"), "{text}");
}

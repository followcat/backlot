//! The Hollywood layer's App wiring tests (H4): the `[hollywood]`
//! gate on `alt+y`, the three-state cycle, the no-resize guarantee
//! and the absent-table parity. Keys live in `keys`, actions in
//! `actions`, mouse in `mouse`. Neutral fixtures only (`acme-shop`,
//! `beta-tools`, `alice`, `host-a`).
mod actions;
mod keys;
mod mouse;

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
    app.sync_target();
    app.refresh_projection();
    app
}

/// The document plus a configured layer — collapsed already.
fn app_with_hollywood() -> App {
    let mut app = app_with_doc();
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
    crate::hollywood::HollywoodRunner,
) {
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    let runner: crate::hollywood::HollywoodRunner = std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(String::new()) })
    });
    (recorded, runner)
}

// ---------- parity (D6) ----------

#[tokio::test]
async fn without_the_table_alt_y_reaches_the_pane_and_no_cell_changes() {
    let mut app = app_with_doc();
    app.area = (120, 24);
    // `Ok(None)` — the absent read the cli path hands the layer.
    app.set_hollywood(Ok(None), std::path::Path::new("config.toml"));
    assert_eq!(app.hollywood.mode, LayerMode::Hidden);
    assert!(!app.hollywood.visible());
    // Alt+Y is not the layer's: it reaches the focused program.
    let before = app.pane.input_backlog().0;
    alt_y(&mut app);
    assert!(app.pane.input_backlog().0 > before, "alt+y was consumed");
    assert_eq!(app.hollywood.mode, LayerMode::Hidden);
    // The frame is the frame herdr draws: no strip on the top row.
    let backend = draw_to(&mut app, 120, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(!text.contains("hollywood"), "{text}");
    let backend2 = draw_to(&mut app, 120, 24);
    assert_eq!(backend.buffer(), backend2.buffer());
}

#[tokio::test]
async fn a_malformed_table_stays_hidden_and_notices() {
    let mut app = app_with_doc();
    app.area = (120, 24);
    app.set_hollywood(
        Err("config.toml: malformed at line 2 column 1".into()),
        std::path::Path::new("config.toml"),
    );
    assert_eq!(app.hollywood.mode, LayerMode::Hidden);
    assert!(app.notice.is_some(), "the bad read notices once");
    let before = app.pane.input_backlog().0;
    alt_y(&mut app);
    assert!(app.pane.input_backlog().0 > before);
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
            );
            assert_eq!(
                app.chrome_layout(cols, 24),
                direct,
                "cols {cols} mode {mode:?}"
            );
        }
    }
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
async fn the_strip_draws_at_the_top_right_and_expands_on_click() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    let backend = draw_to(&mut app, 120, 24);
    let top = &screen_lines(&backend)[0];
    assert!(top.contains("✦ hollywood"), "{top}");
    assert!(
        top.contains("2 live 1 fin") || top.contains("1 live 1 fin"),
        "{top}"
    );
    let strip = app.hollywood_hits.area;
    assert_eq!(strip.height, 1);
    assert_eq!(strip.x + strip.width, 120);
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

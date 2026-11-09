//! The studio panel's App wiring tests: geometry at every width, the
//! `prefix+y` toggle and the closed-panel identity guarantee. Focused
//! keys live in `keys`, mouse in `mouse`. Neutral fixtures only
//! (`acme-shop`, `alice`).
mod keys;
mod mouse;

use super::*;
use crate::app::input::{MouseButton, MouseEventKind, MouseInput, UiEvent};
use crate::panel::{PanelMode, Target};
use hc_proto::watch::{DocKey, Document};

fn summary_json() -> serde_json::Value {
    serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "studios": [{
            "id": "acme-shop", "title": "Acme shop", "state": "live",
            "requirement_version": 1, "mode": "standard",
            "open_reviews": 0, "waiting": 0,
            "roles": [
                { "role": "producer", "name": "producer", "terminal": "t1",
                  "state": "live", "work": "working" },
                { "role": "director", "name": "director", "terminal": "t2",
                  "state": "live", "work": "waiting" },
                { "role": "writer", "name": "writer", "terminal": "t3",
                  "state": "live" },
                { "role": "actor-alice", "name": "alice", "terminal": "t4",
                  "runner": "agent-a", "model": "m", "state": "live",
                  "work": "working", "task": "task-1",
                  "priority": "normal", "doing": "writes code" },
                { "role": "actor-bob", "name": "bob", "terminal": "t5",
                  "state": "live", "work": "blocked", "task": "task-2",
                  "priority": "low" },
                { "role": "actor-dave", "name": "dave",
                  "state": "closed", "resumable": true },
                { "role": "actor-eve", "name": "eve",
                  "state": "closed", "resumable": false }
            ]
        }]
    })
}

fn app_with_panel_doc() -> App {
    let mut app = app_with(&["t1", "t2", "t3", "t4", "t5"]);
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    app.view.inventory.replace(
        2,
        app.view
            .inventory
            .terminals()
            .values()
            .cloned()
            .chain(std::iter::once(stage))
            .collect(),
    );
    app.view.documents.insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        Document {
            value: summary_json(),
            ..Document::default()
        },
    );
    app.focused = Some("t1".into());
    app.sync_target();
    app
}

fn key(app: &mut App, k: hc_vt::Key) {
    app.on_input(
        UiEvent::Key(term_key(k, hc_vt::Mods::NONE, hc_vt::Action::Press)),
        app.area.0,
    );
}

fn prefix(app: &mut App) {
    app.on_input(
        UiEvent::Key(term_key(
            hc_vt::Key::Char('b'),
            hc_vt::Mods::CTRL,
            hc_vt::Action::Press,
        )),
        app.area.0,
    );
}

fn open_panel(app: &mut App) {
    prefix(app);
    key(app, hc_vt::Key::Char('y'));
    assert_eq!(app.panel.mode, PanelMode::Expanded);
    assert!(app.panel.focused);
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

/// An overflowing document: twenty extra waiting actors.
fn overflow_doc() -> Document {
    let mut v = summary_json();
    let roles = v["studios"][0]["roles"].as_array_mut().unwrap();
    for i in 0..20 {
        roles.push(serde_json::json!({
            "role": format!("actor-x{i}"), "name": format!("x{i}"),
            "state": "live", "work": "waiting",
        }));
    }
    Document {
        value: v,
        ..Document::default()
    }
}

fn doc_key() -> DocKey {
    DocKey {
        machine: None,
        name: "backlot".into(),
    }
}

// ---------- geometry ----------

#[tokio::test]
async fn a_closed_panel_reserves_nothing_at_any_width() {
    let mut app = app_with_panel_doc();
    for cols in [200u16, 120, 80, 64, 40] {
        app.area = (cols, 24);
        let direct = app.shell.layout_config().layout(
            cols,
            24,
            app.shell_state.sidebar_collapsed,
            crate::app::pane_geometry::focused_tab_count(&app),
            app.sidebar_width,
        );
        let chrome = app.chrome_layout(cols, 24);
        assert_eq!(chrome.pane_surface, direct.pane_surface, "cols {cols}");
        assert_eq!(app.panel_width(), 0);
        assert!(!app.panel_beside(cols));
        assert!(!app.panel_fullscreen());
    }
}

#[tokio::test]
async fn the_beside_panel_shaves_columns_off_the_panes() {
    let mut app = app_with_panel_doc();
    app.panel.mode = PanelMode::Expanded;
    // 200 and 120 leave ≥60 for the panes past the mobile threshold.
    for (cols, pane_w) in [(200u16, 170), (120, 90)] {
        app.area = (cols, 24);
        assert!(app.panel_beside(cols), "cols {cols}");
        let layout = app.chrome_layout(cols, 24);
        let direct = app.shell.layout_config().layout(
            cols - app.panel_width(),
            24,
            app.shell_state.sidebar_collapsed,
            crate::app::pane_geometry::focused_tab_count(&app),
            app.sidebar_width,
        );
        assert_eq!(layout.pane_surface, direct.pane_surface, "cols {cols}");
        assert_eq!(layout.pane_surface.width, pane_w - app.sidebar_width);
        assert!(!app.panel_fullscreen());
    }
    // A collapsed strip is 16 wide.
    app.panel.mode = PanelMode::Collapsed;
    assert_eq!(app.panel_width(), 16);
    assert!(app.panel_beside(120));
}

#[tokio::test]
async fn a_beside_panel_never_flips_the_chrome_to_mobile() {
    let mut app = app_with_panel_doc();
    // 80 − 16 = 64 and 92 − 30 = 62 dip under the 64-column mobile
    // threshold; the window itself is still desktop-width, so the
    // chrome must keep its desktop layout and sidebar.
    for (cols, width) in [(80u16, PanelMode::Collapsed), (92, PanelMode::Expanded)] {
        app.panel.mode = width;
        app.area = (cols, 24);
        assert!(app.panel_beside(cols), "cols {cols}");
        let layout = app.chrome_layout(cols, 24);
        assert!(!layout.sidebar.is_empty(), "cols {cols}: {layout:?}");
        assert!(layout.mobile_header.is_empty(), "cols {cols}: {layout:?}");
        assert_eq!(
            layout.pane_surface.x + layout.pane_surface.width,
            cols - app.panel_width(),
            "the pane surface ends where the panel starts"
        );
    }
    // Closed calls `layout()` exactly as before — at 64 and under the
    // whole window is genuinely mobile.
    app.panel.mode = PanelMode::Closed;
    for cols in [92u16, 80, 64, 40] {
        let direct = app.shell.layout_config().layout(
            cols,
            24,
            app.shell_state.sidebar_collapsed,
            crate::app::pane_geometry::focused_tab_count(&app),
            app.sidebar_width,
        );
        assert_eq!(app.chrome_layout(cols, 24), direct, "cols {cols}");
    }
}

#[tokio::test]
async fn a_panel_that_cannot_fit_draws_full_screen() {
    let mut app = app_with_panel_doc();
    app.panel.mode = PanelMode::Expanded;
    // 80 − 30 = 50 < 60: full-screen. 40 is under the mobile threshold.
    for cols in [80u16, 40] {
        app.area = (cols, 24);
        assert!(!app.panel_beside(cols), "cols {cols}");
        assert!(app.panel_fullscreen(), "cols {cols}");
        // Pane geometry is the panel-closed shape: nothing resizes.
        let direct = app.shell.layout_config().layout(
            cols,
            24,
            app.shell_state.sidebar_collapsed,
            crate::app::pane_geometry::focused_tab_count(&app),
            app.sidebar_width,
        );
        assert_eq!(
            app.chrome_layout(cols, 24).pane_surface,
            direct.pane_surface
        );
    }
}

// ---------- toggle ----------

#[tokio::test]
async fn the_toggle_cycle_is_closed_then_focused_then_closed() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    // Open + focused → the chord closes.
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(app.panel.mode, PanelMode::Closed);
    assert!(!app.panel.focused);
    // Re-open, unfocus via Esc, then the chord refocuses.
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.panel.focused);
    assert_eq!(app.panel.mode, PanelMode::Expanded);
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('y'));
    assert!(app.panel.focused);
}

// ---------- closed-panel identity ----------

#[tokio::test]
async fn a_closed_panel_is_identical_in_geometry_keys_and_cells() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    // Geometry: the helper agrees with a direct call at every width.
    for cols in [200u16, 120, 80, 40] {
        let direct = app.shell.layout_config().layout(
            cols,
            24,
            app.shell_state.sidebar_collapsed,
            crate::app::pane_geometry::focused_tab_count(&app),
            app.sidebar_width,
        );
        assert_eq!(
            app.chrome_layout(cols, 24).pane_surface,
            direct.pane_surface
        );
    }
    // A plain key still forwards to the terminal untouched.
    let before = app.pane.input_backlog().0;
    key(&mut app, hc_vt::Key::Char('x'));
    assert!(app.pane.input_backlog().0 > before);
    // The frame a closed panel draws is the frame herdr draws: the
    // right-hand columns carry the pane, not panel chrome.
    let backend = draw_to(&mut app, 120, 24);
    let lines = screen_lines(&backend);
    let right: String = lines
        .iter()
        .map(|l| l.chars().rev().take(16).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!right.contains("acme-shop"), "{right}");
    assert!(!right.contains("working:"), "{right}");
    // And a second draw is byte-identical: no panel state leaks a frame.
    let backend2 = draw_to(&mut app, 120, 24);
    assert_eq!(backend.buffer(), backend2.buffer());
}

#[tokio::test]
async fn an_open_panel_draws_its_column_and_fullscreen_covers() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    let backend = draw_to(&mut app, 120, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("working: producer, actors"));
    assert!(text.contains("acme-shop"));
    // At 80 the same panel covers the frame instead.
    app.area = (80, 24);
    let backend = draw_to(&mut app, 80, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("acme-shop"), "{text}");
}

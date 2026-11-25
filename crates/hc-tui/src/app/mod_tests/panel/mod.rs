//! The studio panel's App wiring tests: geometry at every width, the
//! `prefix+y` toggle and the closed-panel identity guarantee. Focused
//! keys live in `keys`, mouse in `mouse`. Neutral fixtures only
//! (`acme-shop`, `alice`).
mod compose;
mod draw;
mod exchange;
mod focus_lot;
mod focus_lot_hints;
mod input;
mod keys;
mod menus;
mod merge;
mod mouse;
mod reader_actions;
mod revise;
mod sheet;
mod stage;

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
    app.set_plain(false);
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    let terminals: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .cloned()
        .chain(std::iter::once(stage))
        .collect();
    app.view.inventory_mut().replace(2, terminals);
    app.view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        Document {
            value: summary_json(),
            ..Document::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    app.sync_target();
    // Latch studio mode the way a publish does: active + current.
    app.reconcile_published_view();
    // U7: these tests measure the cast and the panel; the Hollywood
    // bar would share the top rows, so it is hidden — and hidden
    // after the publish, which is what opens a bar on a client with
    // studios.
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    app
}

/// `host-b`'s `backlot` document: `beta-tools` with a live
/// producer/director and `actor-a` carrying `t1` at high priority.
/// Bare terminals — `parse` qualifies them to `host-b/<t>`.
fn remote_summary_json() -> serde_json::Value {
    serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 2,
        "studios": [{
            "id": "beta-tools", "title": "Beta tools", "state": "live",
            "requirement_version": 1, "mode": "standard",
            "open_reviews": 0, "waiting": 0,
            "next": "deliver t1",
            "roles": [
                { "role": "producer", "name": "producer", "terminal": "t1",
                  "state": "live" },
                { "role": "director", "name": "director", "terminal": "t2",
                  "state": "live" },
                { "role": "actor-a", "name": "actor-a", "terminal": "t10",
                  "runner": "agent-a", "model": "m", "state": "live",
                  "task": "t1", "priority": "high", "doing": "writes code" }
            ]
        }]
    })
}

/// A two-machine app: the local `acme-shop` plus `host-b`'s
/// `beta-tools`, both stages live — the shape the real check drove.
fn app_with_two_machines() -> App {
    let mut app = app_with_panel_doc();
    app.view.machines = Some(vec![hc_proto::machine::MachineView {
        name: "host-b".into(),
        target: "host-b".into(),
        id: "id-host-b".into(),
        host: "host-b".into(),
        hostname: None,
        version: "0".into(),
        state: "connected".into(),
        session: None,
        since_ms: None,
        temporary: false,
        via: false,
        why: None,
    }]);
    app.view.documents_mut().insert(
        DocKey {
            machine: Some("host-b".into()),
            name: "backlot".into(),
        },
        Document {
            value: remote_summary_json(),
            ..Document::default()
        }
        .into(),
    );
    let mut stage = crate::sidebar::fixture::term("host-b/t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    let mut terms: Vec<_> = app.view.inventory.terminals().values().cloned().collect();
    terms.push(stage);
    for address in ["host-b/t1", "host-b/t2", "host-b/t10"] {
        terms.push(crate::sidebar::fixture::term(address));
    }
    app.view.inventory_mut().replace(3, terms);
    app.reconcile_published_view();
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    app
}

/// A `terminal.run`-recording panel runner — the local dock's
/// `backlot …` argv capture.
fn runner() -> (
    std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    crate::app::action_tracker::TestPanelRunner,
) {
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    (
        recorded,
        std::sync::Arc::new(move |argv: Vec<String>| {
            sink.lock().unwrap().push(argv);
            Box::pin(async { Ok(()) })
        }),
    )
}

/// The studio's parked draft — what its `›` row would paint.
fn draft(app: &App, key: &str) -> String {
    app.panel
        .draft(&key.into())
        .map(|draft| draft.as_str().to_owned())
        .unwrap_or_default()
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

// ---------- default ----------

#[tokio::test]
async fn the_dock_opens_expanded_on_launch() {
    // `app()` runs --plain; `app_with_plain(false)` is the launch
    // shape a real run gets.
    let mut app = app_with_plain(false);
    app.area = (120, 24);
    assert_eq!(app.panel.mode, PanelMode::Expanded);
    assert!(app.panel.open());
    assert!(app.panel_beside(120));
    assert_eq!(app.panel_width(120), 36);
    // --plain still buys the plain multiplexer's frame outright.
    app.set_plain(true);
    assert_eq!(app.panel.mode, PanelMode::Closed);
    assert!(!app.panel.open());
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
            false,
        );
        let chrome = app.chrome_layout(cols, 24);
        assert_eq!(chrome.pane_surface, direct.pane_surface, "cols {cols}");
        assert_eq!(app.panel_width(cols), 0);
        assert!(!app.panel_beside(cols));
        assert!(!app.panel_fullscreen());
    }
}

#[tokio::test]
async fn the_beside_panel_shaves_columns_off_the_panes() {
    let mut app = app_with_panel_doc();
    app.panel.mode = PanelMode::Expanded;
    // 200 and 120 leave ≥44 for the panes past the sidebar and dock.
    for cols in [200u16, 120] {
        app.area = (cols, 24);
        assert!(app.panel_beside(cols), "cols {cols}");
        let layout = app.chrome_layout(cols, 24);
        let direct = app.shell.layout_config().layout(
            cols - app.panel_width(cols),
            24,
            app.shell_state.sidebar_collapsed,
            crate::app::pane_geometry::focused_tab_count(&app),
            app.sidebar_width,
            false,
        );
        assert_eq!(layout.pane_surface, direct.pane_surface, "cols {cols}");
        let region = cols - app.panel_width(cols);
        assert_eq!(layout.pane_surface.width, region - app.sidebar_width);
        assert!(!app.panel_fullscreen());
    }
    // The spec's dock-width tiers when wide enough to sit beside:
    // 38 at 140+, 36 at 100..=139. Below 100 (or when panes+dock
    // cannot fit), an unfocused dock auto-hides to 0 columns.
    assert_eq!(app.panel_width(160), 38);
    assert_eq!(app.panel_width(120), 36);
    assert_eq!(app.panel_width(99), 0);
    assert_eq!(app.panel_width(80), 0);
    // A collapsed strip is 16 wide when >= 100 cols.
    app.panel.mode = PanelMode::Collapsed;
    assert_eq!(app.panel_width(120), 16);
    assert!(app.panel_beside(120));
    assert_eq!(app.panel_width(80), 0);
    assert!(!app.panel_beside(80));
}

#[tokio::test]
async fn a_beside_panel_never_flips_the_chrome_to_mobile() {
    let mut app = app_with_panel_doc();
    // At 100 cols with a collapsed 4-col sidebar and 36-col expanded
    // dock, 100 − 36 = 64 dips to the 64-column mobile threshold; the
    // window itself is still desktop-width, so the chrome must keep
    // its desktop layout and sidebar.
    app.shell_state.sidebar_collapsed = true;
    for (cols, mode) in [(100u16, PanelMode::Collapsed), (100, PanelMode::Expanded)] {
        app.panel.mode = mode;
        app.area = (cols, 24);
        assert!(app.panel_beside(cols), "cols {cols}");
        let layout = app.chrome_layout(cols, 24);
        assert!(!layout.sidebar.is_empty(), "cols {cols}: {layout:?}");
        assert!(layout.mobile_header.is_empty(), "cols {cols}: {layout:?}");
        assert_eq!(
            layout.pane_surface.x + layout.pane_surface.width,
            cols - app.panel_width(cols),
            "the pane surface ends where the panel starts"
        );
    }
    app.shell_state.sidebar_collapsed = false;
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
            false,
        );
        assert_eq!(app.chrome_layout(cols, 24), direct, "cols {cols}");
    }
}

/// Below 100 columns (`R3-1` / `R3-M3`), an unfocused dock hides
/// completely (`0` columns) so panes keep full width, while `panel.mode`
/// remembers `Expanded` and automatically restores the beside column
/// when widened back to `>= 100`. Focusing (`Ctrl+B y`) opens the
/// full `Expanded` dock as a `pane_surface` overlay (`R3-B4`), never
/// downgraded to `Collapsed`.
#[tokio::test]
async fn a_too_narrow_terminal_hides_unfocused_and_overlays_focused() {
    let mut app = app_with_panel_doc();
    app.panel.mode = PanelMode::Expanded;
    // At 80 cols unfocused: 0 columns reserved, pane gets full 80 - sidebar.
    app.area = (80, 24);
    assert_eq!(app.panel_mode_at(80), PanelMode::Closed);
    assert!(!app.panel_beside(80));
    assert_eq!(app.panel_width(80), 0);
    assert_eq!(app.dock_reserve(80), 0);
    assert!(!app.panel_fullscreen());
    // When focused at 80: stays Expanded and overlays inside pane_surface (>= 36 cols).
    app.panel.focused = true;
    assert_eq!(app.panel_mode_at(80), PanelMode::Expanded);
    assert!(!app.panel_beside(80));
    assert!(app.panel_fullscreen());
    let layout80 = app.chrome_layout(80, 24);
    let overlay80 = app.panel_overlay_rect(layout80.pane_surface, 80);
    assert_eq!(overlay80.width, 36);
    assert!(overlay80.x >= layout80.pane_surface.x);
    // At 60 (mobile) focused: full pane_surface width (60 cols).
    app.area = (60, 20);
    assert!(!app.panel_beside(60));
    let layout60 = app.chrome_layout(60, 20);
    assert!(layout60.sidebar.is_empty());
    assert_eq!(layout60.pane_surface.width, 60);
    assert_eq!(app.panel_overlay_rect(layout60.pane_surface, 60).width, 60);
    // Unfocusing and widening restores the beside column cleanly.
    app.panel.focused = false;
    assert_eq!(app.panel.mode, PanelMode::Expanded);
    assert!(app.panel_beside(120));
    assert_eq!(app.panel_width(120), 36);
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
            false,
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
    assert!(!right.contains("lot"), "{right}");
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
    assert!(text.contains("lot · 1"), "{text}");
    assert!(text.contains("acme-shop"));
    // At 80 the same panel covers the frame instead.
    app.area = (80, 24);
    let backend = draw_to(&mut app, 80, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("acme-shop"), "{text}");
}

#[tokio::test]
async fn mobile_60x20_activates_mobile_header_full_pane_and_full_width_overlay() {
    let mut app = app_with_panel_doc();
    app.area = (60, 20);
    // Unfocused default at 60x20: mobile header active, sidebar hidden,
    // dock hidden (0 cols), pane gets all 60 columns.
    assert!(!app.panel_beside(60));
    assert_eq!(app.panel_mode_at(60), PanelMode::Closed);
    assert_eq!(app.dock_reserve(60), 0);
    assert_eq!(app.sidebar_render_width(60), 0);
    let layout = app.chrome_layout(60, 20);
    assert!(layout.sidebar.is_empty());
    assert!(layout.mobile_header.height > 0);
    assert_eq!(layout.pane_surface.width, 60);

    let drawn = draw_to(&mut app, 60, 20);
    let header_row = screen_lines(&drawn)[0].clone();
    assert!(header_row.contains("acme-shop"), "{header_row}");
    assert!(header_row.contains("producer"), "{header_row}");
    assert!(header_row.contains("✦1"), "{header_row}");
    let header_area: String = screen_lines(&drawn)[..2].join("\n");
    assert!(header_area.contains("switch"), "{header_area}");

    // `Ctrl+B y` opens the full-width 60-col Expanded overlay inside pane_surface.
    open_panel(&mut app);
    assert!(app.panel_fullscreen());
    assert_eq!(
        app.panel_overlay_rect(layout.pane_surface, 60),
        layout.pane_surface
    );
    let open = draw_to(&mut app, 60, 20);
    let text = screen_lines(&open).join("\n");
    assert!(text.contains("lot · 1"), "{text}");
    assert!(text.contains("alice · task-1"), "{text}");
    assert!(text.contains("tell producer"), "{text}");
    // Mobile header stays visible on row 0 above the overlay.
    assert!(screen_lines(&open)[0].contains("acme-shop"), "{text}");

    // `Esc` closes back to the 60-col pane; a second `Ctrl+B y` re-opens cleanly.
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.panel.mode, PanelMode::Closed);
    assert!(!app.panel_fullscreen());
    assert!(!app.panel_focused());
    open_panel(&mut app);
    assert_eq!(app.panel.mode, PanelMode::Expanded);
    assert!(app.panel.focused);
}

#[tokio::test]
async fn narrow_80x24_hides_unfocused_and_opens_expanded_overlay_on_toggle() {
    let mut app = app_with_panel_doc();
    app.sidebar_width = 25;
    app.area = (80, 24);
    // Unfocused default at 80x24: dock auto-hides to 0 cols; pane gets 55 cols.
    assert_eq!(app.panel_mode_at(80), PanelMode::Closed);
    assert_eq!(app.dock_reserve(80), 0);
    let layout = app.chrome_layout(80, 24);
    assert_eq!(layout.sidebar.width, 25);
    assert_eq!(layout.pane_surface.width, 55);

    // `Ctrl+B y` opens Expanded overlay (>= 36 cols) inside the 55-col pane_surface,
    // leaving the 25-col sidebar untouched.
    open_panel(&mut app);
    assert_eq!(app.panel_mode_at(80), PanelMode::Expanded);
    assert!(app.panel_fullscreen());
    let overlay = app.panel_overlay_rect(layout.pane_surface, 80);
    assert!(overlay.width >= 36, "overlay {overlay:?}");
    assert!(overlay.x >= layout.pane_surface.x, "overlay {overlay:?}");
    assert_eq!(overlay.x + overlay.width, 80);

    let open = draw_to(&mut app, 80, 24);
    let text = screen_lines(&open).join("\n");
    assert!(text.contains("alice · task-1"), "{text}");
    assert!(text.contains("tell producer"), "{text}");
    // Space drawer works inside the 80x24 Expanded overlay.
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    key(&mut app, hc_vt::Key::Char(' '));
    assert!(app.panel.drawer.is_some());

    // `Esc` closes back to 0 cols; second `Ctrl+B y` re-opens cleanly.
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.panel_mode_at(80), PanelMode::Closed);
    assert!(!app.panel.focused);
    open_panel(&mut app);
    assert_eq!(app.panel_mode_at(80), PanelMode::Expanded);
    assert!(app.panel.focused);

    // `Ctrl+B y` while overlay open closes it too; widening unfocused from 80 -> 120
    // restores the beside 36-col Expanded column when mode remained Expanded.
    key(&mut app, hc_vt::Key::Escape);
    app.panel.mode = PanelMode::Expanded;
    app.panel.focused = false;
    assert_eq!(app.panel_mode_at(80), PanelMode::Closed);
    app.area = (120, 40);
    assert_eq!(app.panel_mode_at(120), PanelMode::Expanded);
    assert!(app.panel_beside(120));
    assert_eq!(app.panel_width(120), 36);
}

#[tokio::test]
async fn narrow_80x24_opening_page_renders_47_col_backlot_frame() {
    let mut app = app_with(&["t1"]);
    app.set_plain(false);
    app.sidebar_width = 25;
    app.focused = Some("t1".into());
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: serde_json::json!({
                "schema": "backlot/1",
                "updated_ms": 2,
                "studios": []
            }),
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.area = (80, 24);
    assert!(app.opening_visible());
    let layout = app.chrome_layout(80, 24);
    assert_eq!(layout.pane_surface.width, 55);
    let drawn = draw_to(&mut app, 80, 24);
    let text = screen_lines(&drawn).join("\n");
    assert!(
        text.contains("█▀▄"),
        "47-col BACKLOT art frame fits in 55-col pane:\n{text}"
    );
    assert!(text.contains("a film studio for coding agents"), "{text}");
}

#[tokio::test]
async fn manual_c_strip_at_120x40_shows_calm_summary_for_idle_studio() {
    let mut app = app_with(&["t1"]);
    app.set_plain(false);
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    let terminals: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .cloned()
        .chain(std::iter::once(stage))
        .collect();
    app.view.inventory_mut().replace(2, terminals);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: serde_json::json!({
                "schema": "backlot/1",
                "updated_ms": 2,
                "studios": [{
                    "id": "acme-shop",
                    "title": "Acme shop",
                    "state": "live",
                    "requirement_version": 1,
                    "mode": "standard",
                    "open_reviews": 0,
                    "waiting": 0,
                    "tasks_done": 1,
                    "tasks_total": 5,
                    "roles": [
                        { "role": "producer", "name": "producer", "terminal": "t1", "state": "live", "work": "waiting" },
                        { "role": "director", "name": "director", "state": "live", "work": "waiting" },
                        { "role": "actor-a", "name": "alice", "state": "closed", "resumable": true }
                    ]
                }]
            }),
            ..Document::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    app.area = (120, 40);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('c'));
    assert_eq!(app.panel.mode, PanelMode::Collapsed);
    assert!(app.panel_beside(120));
    assert_eq!(app.panel_width(120), 16);
    let drawn = draw_to(&mut app, 120, 40);
    let text = screen_lines(&drawn).join("\n");
    assert!(text.contains("◦2 ✓1"), "{text}");
}

#[tokio::test]
async fn plain_mode_is_unaffected_at_120_80_and_60() {
    let mut app = app_with_plain(true);
    for (cols, rows) in [(120u16, 40u16), (80, 24), (60, 20)] {
        app.area = (cols, rows);
        assert_eq!(app.panel.mode, PanelMode::Closed);
        assert!(!app.panel_beside(cols));
        assert_eq!(app.panel_width(cols), 0);
        let direct = app.shell.layout_config().layout(
            cols,
            rows,
            app.shell_state.sidebar_collapsed,
            crate::app::pane_geometry::focused_tab_count(&app),
            app.sidebar_width,
            false,
        );
        assert_eq!(app.chrome_layout(cols, rows), direct, "cols {cols}");
    }
}

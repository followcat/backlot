//! Group 2 fail-first tests: Single-Focus-Ring invariant (§1.2.5),
//! muted Terminal border + `[unfocused · Esc returns]`, Lot heavy
//! `┏`/`┃` focus chrome, faint `·` remembered cursors, inverted
//! `« chip »` action pills, 1-line hints, `FOCUS:` modebar breadcrumb,
//! and 200x44 / 100x40 text-buffer visual snapshots (§1.5).

use super::*;
use crate::app::input::UiEvent;
use crate::focus::{LotLevel, WorkspaceRegion};
use hc_proto::watch::{DocKey, Document};

fn key(app: &mut App, k: hc_vt::Key) {
    key_mods(app, k, hc_vt::Mods::NONE);
}

fn key_mods(app: &mut App, k: hc_vt::Key, mods: hc_vt::Mods) {
    app.on_input(
        UiEvent::Key(term_key(k, mods, hc_vt::Action::Press)),
        app.area.0,
    );
}

fn alt_l(app: &mut App) {
    key_mods(app, hc_vt::Key::Char('l'), hc_vt::Mods::ALT);
}

fn prefix_key(app: &mut App, ch: char) {
    key_mods(app, hc_vt::Key::Char('b'), hc_vt::Mods::CTRL);
    key(app, hc_vt::Key::Char(ch));
}

fn visual_app() -> App {
    let mut app = app_with(&["t1", "t2", "t3", "t7"]);
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
    let doc = serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "hollywood": { "state": "live", "runner": "fake", "model": "m", "effort": "high" },
        "studios": [
            {
                "id": "acme-shop",
                "title": "Acme shop",
                "state": "live",
                "requirement_version": 2,
                "mode": "standard",
                "open_reviews": 1,
                "waiting": 1,
                "tasks_done": 1,
                "tasks_total": 3,
                "roles": [
                    {
                        "role": "producer", "name": "producer", "terminal": "t1",
                        "state": "live", "work": "waiting",
                        "asks": "Ship OAuth v2 or keep v1?", "asks_seq": 48, "since_ms": 1_000
                    },
                    {
                        "role": "director", "name": "director", "terminal": "t2",
                        "state": "live", "work": "working"
                    },
                    {
                        "role": "actor-t1", "name": "actor-t1", "terminal": "t3",
                        "state": "live", "work": "working", "task": "t2"
                    }
                ],
                "tasks": [
                    {
                        "id": "t1", "title": "auth middleware", "status": "accepted",
                        "priority": "normal", "requirement_version": 2,
                        "rework_rounds": 0, "rework_cap": 3
                    },
                    {
                        "id": "t2", "title": "checkout webhooks", "status": "running",
                        "priority": "normal", "requirement_version": 2,
                        "rework_rounds": 0, "rework_cap": 3
                    }
                ],
                "exchange": [
                    { "seq": 10, "who": "user", "text": "Prioritize v2 schema." }
                ]
            },
            {
                "id": "beta-api",
                "title": "Beta API",
                "state": "live",
                "requirement_version": 1,
                "mode": "standard",
                "open_reviews": 0,
                "waiting": 0,
                "roles": [
                    { "role": "producer", "name": "beta-prod", "terminal": "t7", "state": "live", "work": "waiting" }
                ]
            }
        ]
    });
    app.view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    app.area = (200, 44);
    app
}

#[tokio::test]
async fn single_focus_ring_invariant_and_breadcrumb_200x44() {
    let mut app = visual_app();
    app.area = (200, 44);

    // 1. Terminal focused: no FOCUS breadcrumb, no heavy `┏`, no dimmed `[unfocused · Esc returns]`.
    let term_buf = draw_to(&mut app, 200, 44);
    let term_screen = screen_lines(&term_buf).join("\n");
    assert!(
        !term_screen.contains("FOCUS:"),
        "Terminal focus shows no FOCUS breadcrumb:\n{term_screen}"
    );
    assert!(
        !term_screen.contains("[unfocused"),
        "Terminal focus shows no unfocused tag:\n{term_screen}"
    );

    // 2. Enter Lot inside acme-shop via Alt+L:
    //    - pane border carries `[unfocused · Esc returns]` and drops accent highlight
    //    - Lot draws heavy `┏` focus frame & `┃` accent left rail & `entered studio`
    //    - bottom modebar shows `FOCUS: lot › acme-shop`
    alt_l(&mut app);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::InsideStudio("acme-shop".into()))
    );
    let lot_buf = draw_to(&mut app, 200, 44);
    let lot_text = screen_lines(&lot_buf).join("\n");
    assert!(
        lot_text.contains("[unfocused · Esc returns]"),
        "pane top border shows unfocused marker:\n{lot_text}"
    );
    assert!(
        lot_text.contains('┏') && lot_text.contains('┃'),
        "Lot shows heavy accent focus chrome:\n{lot_text}"
    );
    assert!(
        lot_text.contains("FOCUS: lot › acme-shop"),
        "modebar shows FOCUS breadcrumb:\n{lot_text}"
    );

    // Enter RowActions with ArrowRight: highlighted chip draws `« ... »` inverted pill.
    key(&mut app, hc_vt::Key::ArrowRight);
    let chip_text = screen_lines(&draw_to(&mut app, 200, 44)).join("\n");
    assert!(
        chip_text.contains('«') && chip_text.contains('»'),
        "active action chip draws inverted pill delimiters:\n{chip_text}"
    );

    // 3. Switch to Tree (`Ctrl+B t`) and Cast (`Ctrl+B a`): breadcrumb updates cleanly
    //    and Esc back to Terminal clears `FOCUS:`.
    prefix_key(&mut app, 't');
    let tree_text = screen_lines(&draw_to(&mut app, 200, 44)).join("\n");
    assert!(
        tree_text.contains("FOCUS: tree"),
        "Tree focus shows FOCUS: tree breadcrumb:\n{tree_text}"
    );

    key(&mut app, hc_vt::Key::Tab);
    let cast_text = screen_lines(&draw_to(&mut app, 200, 44)).join("\n");
    assert!(
        cast_text.contains("FOCUS: cast"),
        "Cast focus shows FOCUS: cast breadcrumb:\n{cast_text}"
    );

    key(&mut app, hc_vt::Key::Escape);
    let back_text = screen_lines(&draw_to(&mut app, 200, 44)).join("\n");
    assert!(
        !back_text.contains("FOCUS:"),
        "Esc to Terminal hides FOCUS breadcrumb:\n{back_text}"
    );
}

#[tokio::test]
async fn narrow_overlay_focus_visuals_100x40() {
    let mut app = visual_app();
    app.area = (100, 40);
    app.shell_state.sidebar_collapsed = true;
    app.panel.sidebar_collapsed = true;

    alt_l(&mut app);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::InsideStudio("acme-shop".into()))
    );
    let buf = draw_to(&mut app, 100, 40);
    let screen = screen_lines(&buf).join("\n");
    assert!(
        screen.contains("FOCUS: lot › acme-shop"),
        "narrow overlay still displays modebar breadcrumb:\n{screen}"
    );
    assert!(
        screen.contains('┏') && screen.contains('┃'),
        "narrow Lot overlay keeps heavy focus frame and left rail:\n{screen}"
    );
    assert!(
        screen.contains("entered studio"),
        "narrow Lot overlay displays entered-studio header cue:\n{screen}"
    );
}

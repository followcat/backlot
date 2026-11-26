//! `Alt+PgUp`/`Alt+PgDn` also switch the main pane: the studio stepped to
//! shows the member it was last left on, by the same fallback order
//! `focus_studio` uses — remembered member, producer, first live person,
//! first live shell. A step from a terminal moves the keys to the landed
//! member's terminal; a step from the lot or the machines tree swaps the
//! pane but leaves that surface focused, its selection on the studio.
//! A settled studio with no live member leaves the pane as it is, quietly.
//! Neutral fixtures only (`acme-shop`, `night-owl`, `ghost`).

use super::*;
use crate::app::input::UiEvent;
use crate::panel::PanelMode;
use crate::studio::StudioKey;
use hc_proto::watch::Document;

/// `acme-shop`: producer `t1`, actor alice `t4`. `night-owl`: producer
/// `t7`, writer `t8`. `ghost`: one closed seat — settled, nobody live.
fn follow_json() -> serde_json::Value {
    serde_json::json!({
        "schema": "backlot/1", "updated_ms": 1,
        "studios": [
            {
                "id": "acme-shop", "title": "Acme shop", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "roles": [
                    { "role": "producer", "name": "producer", "terminal": "t1",
                      "state": "live", "work": "working" },
                    { "role": "actor-alice", "name": "alice", "terminal": "t4",
                      "state": "live", "work": "working", "task": "task-1" }
                ]
            },
            {
                "id": "night-owl", "title": "Night owl", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "roles": [
                    { "role": "producer", "name": "owl-producer", "terminal": "t7",
                      "state": "live", "work": "waiting" },
                    { "role": "writer", "name": "owl-writer", "terminal": "t8",
                      "state": "live", "work": "working" }
                ]
            },
            {
                "id": "ghost", "title": "Ghost", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "roles": [
                    { "role": "actor-eve", "name": "eve",
                      "state": "closed", "resumable": true }
                ]
            }
        ]
    })
}

fn follow_app() -> App {
    let mut app = app_with(&["t1", "t2", "t3", "t4", "t5", "t6", "t7", "t8", "t99"]);
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
        .chain([stage])
        .collect();
    app.view.inventory_mut().replace(2, terminals);
    app.view.documents_mut().insert(
        super::studio::doc_key(),
        Document {
            value: follow_json(),
            ..Document::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    app
}

fn alt(app: &mut App, k: hc_vt::Key) {
    super::studio::alt(app, k);
}

fn press_tree_chord(app: &mut App) {
    for key in [
        term_key(
            hc_vt::Key::Char('b'),
            hc_vt::Mods::CTRL,
            hc_vt::Action::Press,
        ),
        term_key(
            hc_vt::Key::Char('t'),
            hc_vt::Mods::NONE,
            hc_vt::Action::Press,
        ),
    ] {
        app.on_input(UiEvent::Key(key), app.area.0);
    }
}

// ---------- the landed member ----------

#[tokio::test]
async fn a_step_lands_the_remembered_member_of_the_studio_it_reaches() {
    let mut app = follow_app();
    // Leave night-owl on its writer, then go back to acme-shop's
    // producer: each studio remembers the person it was left on.
    app.focus_pane("t8".into());
    assert_eq!(app.studio.current, Some("night-owl".into()));
    app.focus_pane("t1".into());
    assert_eq!(app.studio.current, Some("acme-shop".into()));
    assert_eq!(
        app.studio
            .remembered
            .get(&StudioKey::local("night-owl"))
            .map(String::as_str),
        Some("writer")
    );

    // `Alt+PgDn` from acme's producer lands night-owl's writer.
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("night-owl".into()));
    assert_eq!(
        app.focused.as_deref(),
        Some("t8"),
        "the remembered writer lands in the main pane"
    );
    assert_eq!(
        app.studio.focus,
        Some(crate::studio::StudioFocus::Role(
            StudioKey::local("night-owl"),
            "t8".into()
        ))
    );

    // Stepping back lands acme's producer.
    alt(&mut app, hc_vt::Key::PageUp);
    assert_eq!(app.studio.current, Some("acme-shop".into()));
    assert_eq!(app.focused.as_deref(), Some("t1"));
}

#[tokio::test]
async fn a_step_with_no_remembered_member_lands_the_producer() {
    let mut app = follow_app();
    app.studio.remembered.clear();
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("night-owl".into()));
    assert_eq!(
        app.focused.as_deref(),
        Some("t7"),
        "the producer is the fallback"
    );
}

#[tokio::test]
async fn repeated_steps_anchor_on_the_studio_just_landed_on() {
    let mut app = follow_app();
    // acme → night-owl → ghost → acme, each step from the last landing.
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("night-owl".into()));
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("ghost".into()));
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("acme-shop".into()), "wraps");
    assert_eq!(app.focused.as_deref(), Some("t1"));
}

// ---------- keyboard ownership ----------

#[tokio::test]
async fn a_step_from_the_lot_keeps_the_lot_focused_on_the_new_studio() {
    let mut app = follow_app();
    app.area = (120, 24);
    app.panel.mode = PanelMode::Expanded;
    app.panel.focused = true;
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("night-owl".into()));
    assert_eq!(app.focused.as_deref(), Some("t7"), "the pane swapped");
    assert!(app.panel_focused(), "the lot kept the keys");
    assert_eq!(
        app.panel.selected.as_ref().and_then(|t| t.studio()),
        Some(&StudioKey::local("night-owl")),
        "the lot's cursor sits on the studio it landed on"
    );
    // Keeping stepping from the lot still works.
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("ghost".into()));
}

#[tokio::test]
async fn a_step_from_the_machines_tree_keeps_the_tree_focused() {
    let mut app = follow_app();
    app.area = (120, 28);
    draw_to(&mut app, 120, 28);
    press_tree_chord(&mut app);
    assert!(app.tree_focused(), "the tree owns the keys");
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("night-owl".into()));
    assert_eq!(app.focused.as_deref(), Some("t7"), "the pane swapped");
    assert!(app.tree_focused(), "the tree kept the keys");
    let row = app.tree_focus.clone().expect("a tree row");
    let on_night_owl = app.tree_rows().iter().position(|r| r == &row).map(|_| row);
    assert!(on_night_owl.is_some());
    assert!(
        matches!(
            on_night_owl.unwrap(),
            crate::chrome::studio_tree::SidebarRow::Studio {
                row: crate::chrome::studio_tree::Row::Studio { ref id, .. },
                ..
            } if id == "night-owl"
        ),
        "the cursor sits on the studio it landed on"
    );
}

// ---------- nobody live ----------

#[tokio::test]
async fn a_settled_studio_with_no_live_member_leaves_the_pane_quietly() {
    let mut app = follow_app();
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.focused.as_deref(), Some("t7"));
    // On to ghost: settled, one closed seat. It becomes current, the
    // pane stays put and nothing is announced.
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("ghost".into()));
    assert_eq!(
        app.focused.as_deref(),
        Some("t7"),
        "the main pane is unchanged"
    );
    assert!(app.notice.is_none(), "stepping is quiet");
    assert!(
        app.ns_pending_focus.is_none(),
        "a settled studio never queues"
    );
}

// ---------- the explicit Enter keeps its notice ----------

#[tokio::test]
async fn enter_on_a_tree_row_still_lands_the_member_and_leaves_the_tree() {
    let mut app = follow_app();
    app.area = (120, 28);
    draw_to(&mut app, 120, 28);
    press_tree_chord(&mut app);
    let row = app
        .tree_rows()
        .into_iter()
        .find(|row| {
            matches!(
                row,
                crate::chrome::studio_tree::SidebarRow::Studio {
                    row: crate::chrome::studio_tree::Row::Studio { id, .. },
                    ..
                } if id == "night-owl"
            )
        })
        .expect("night-owl's studio row");
    app.tree_focus = Some(row);
    app.on_input(
        UiEvent::Key(term_key(
            hc_vt::Key::Enter,
            hc_vt::Mods::NONE,
            hc_vt::Action::Press,
        )),
        app.area.0,
    );
    assert_eq!(app.studio.current, Some("night-owl".into()));
    assert_eq!(
        app.focused.as_deref(),
        Some("t7"),
        "Enter lands the producer (nobody remembered)"
    );
    assert!(!app.tree_focused(), "Enter leaves the tree for the pane");
}

// ---------- the key itself ----------

#[tokio::test]
async fn alt_pagedown_steps_and_lands_the_remembered_member() {
    let mut app = follow_app();
    // Leave night-owl on its writer.
    app.focus_pane("t8".into());
    app.focus_pane("t1".into());
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("night-owl".into()));
    assert_eq!(
        app.focused.as_deref(),
        Some("t8"),
        "the remembered writer lands"
    );
}

#[tokio::test]
async fn alt_left_with_defaults_reaches_the_terminal_and_does_not_step() {
    let mut app = follow_app();
    let before = app.pane.input_backlog().0;
    alt(&mut app, hc_vt::Key::ArrowLeft);
    alt(&mut app, hc_vt::Key::ArrowRight);
    assert_eq!(
        app.studio.current,
        Some("acme-shop".into()),
        "no studio step ran"
    );
    assert_eq!(app.focused.as_deref(), Some("t1"));
    assert_eq!(
        app.pane.input_backlog().0,
        before + 2,
        "the terminal got both keys"
    );
}

#[tokio::test]
async fn a_config_that_keeps_alt_right_still_steps() {
    let mut app = follow_app();
    let dir = crate::test_support::TestDir::new("studio-alt-right");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "[keys]\nnext_studio = \"alt+right\"\n").unwrap();
    let (config, notice) = crate::panel::load(&path);
    assert!(notice.is_none());
    app.set_panel_config(config, notice, &path);
    alt(&mut app, hc_vt::Key::ArrowRight);
    assert_eq!(
        app.studio.current,
        Some("night-owl".into()),
        "the user's alt+right steps studios"
    );
    assert_eq!(app.focused.as_deref(), Some("t7"));
}

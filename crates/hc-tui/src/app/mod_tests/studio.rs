//! U1 studio mode: the four direct keys, the mode latch through real
//! publishes, the cast/`others` mouse path and the sidebar-width
//! switch. Neutral fixtures only (`acme-shop`, `night-owl`, `alice`).
use super::*;
use crate::app::input::UiEvent;
use crate::panel::PanelMode;
use crate::studio::StudioKey;
use crate::test_support::TestDir;
use hc_proto::watch::{DocKey, Document};

pub(super) fn doc_key() -> DocKey {
    DocKey {
        machine: None,
        name: "backlot".into(),
    }
}

fn two_studio_json() -> serde_json::Value {
    serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "studios": [
            {
                "id": "acme-shop", "title": "Acme shop", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "roles": [
                    { "role": "producer", "name": "producer", "terminal": "t1",
                      "state": "live", "work": "working" },
                    { "role": "actor-alice", "name": "alice", "terminal": "t4",
                      "state": "live", "work": "working", "task": "task-1",
                      "runner": "agent-a", "model": "m" },
                    { "role": "actor-dave", "name": "dave",
                      "state": "closed", "resumable": true }
                ]
            },
            {
                "id": "night-owl", "title": "Night owl", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "roles": [
                    { "role": "producer", "name": "owl-producer", "terminal": "t7",
                      "state": "live", "work": "waiting" },
                    { "role": "actor-bob", "name": "bob", "terminal": "t8",
                      "state": "live", "work": "blocked", "task": "task-2" }
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

/// `t1..t9` live plus the stage, two stray agents and a non-studio
/// terminal; the two-studios-plus-ghost document publishes through
/// the real path.
pub(super) fn two_studio_app() -> App {
    let mut app = app_with(&["t1", "t2", "t3", "t4", "t5", "t6", "t7", "t8", "t99"]);
    app.set_plain(false);
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    let mut agent_a = crate::sidebar::fixture::agent("t50", "idle");
    agent_a
        .labels
        .insert("herdr.space".into(), "space-t1".into());
    agent_a.labels.insert("herdr.tab".into(), "tab".into());
    let mut agent_b = crate::sidebar::fixture::agent("t51", "working");
    agent_b
        .labels
        .insert("herdr.space".into(), "space-t2".into());
    agent_b.labels.insert("herdr.tab".into(), "tab".into());
    let terminals: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .cloned()
        .chain([stage, agent_a, agent_b])
        .collect();
    app.view.inventory_mut().replace(2, terminals);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: two_studio_json(),
            ..Document::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    // U7: the bar would share the top row with the cast header these
    // tests read; hide it here, its own tests own it.
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    app
}

pub(super) fn alt(app: &mut App, k: hc_vt::Key) {
    app.on_input(
        UiEvent::Key(term_key(k, hc_vt::Mods::ALT, hc_vt::Action::Press)),
        app.area.0,
    );
}

// ---------- mode latch ----------

#[tokio::test]
async fn a_document_with_studios_latches_studio_mode_and_the_cast() {
    let mut app = two_studio_app();
    assert!(app.studio.active);
    assert_eq!(app.studio.current, Some("acme-shop".into()));
    let backend = draw_to(&mut app, 120, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("acme-shop"), "{text}");
    assert!(text.contains("1/3"), "{text}");
    assert!(text.contains("alice task-1"), "{text}");
    // The panel carries bordered studio blocks.
    app.panel.mode = PanelMode::Expanded;
    let backend = draw_to(&mut app, 120, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("lot · 3"), "{text}");
    assert!(text.contains("╭"), "{text}");
    assert!(text.contains("night-owl"), "{text}");
}

#[tokio::test]
async fn a_missing_or_empty_document_keeps_the_mode() {
    let mut app = two_studio_app();
    assert!(app.studio.active);
    // A malformed read: the document slot vanishes (the watcher's
    // last-good rule keeps the panel's own view; the mode latch reads
    // the same `panel_summary`).
    app.view.documents_mut().remove(&doc_key());
    app.refresh_projection();
    assert!(app.studio.active, "a missing/bad read keeps studio mode");
    // U2: a good document with zero studios keeps it too — the cast
    // is the no-studio view (its `others` row), and the opening page
    // covers the panes. Only `--plain` leaves studio mode.
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: serde_json::json!({
                "schema": "backlot/1", "updated_ms": 2, "studios": []
            }),
            ..Document::default()
        }
        .into(),
    );
    app.refresh_projection();
    assert!(app.studio.active, "zero studios keeps studio mode");
    let backend = draw_to(&mut app, 120, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("cast"), "{text}");
    assert!(text.contains("others"), "{text}");
    assert!(!text.contains("cast ·"), "{text}");
}

#[tokio::test]
async fn the_sidebar_width_follows_the_mode_unless_dragged() {
    // `--plain` keeps herdr's width throughout.
    let mut plain = app();
    plain.refresh_projection();
    let herdr = plain.shell.sidebar_width;
    assert_eq!(plain.sidebar_width, herdr, "--plain keeps herdr's width");
    // Studio mode (non-`--plain`) → backlot's width, documents or not.
    let mut app = two_studio_app();
    assert_eq!(app.sidebar_width, 26);
    app.view.documents_mut().remove(&doc_key());
    app.refresh_projection();
    assert_eq!(app.sidebar_width, 26, "no document keeps backlot's width");
}

// ---------- the four keys ----------

#[tokio::test]
async fn next_and_previous_studio_cycle_and_land_on_the_remembered() {
    let mut app = two_studio_app();
    // Producer-focused → acme remembers producer.
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.studio.current, Some("night-owl".into()));
    assert_eq!(app.focused.as_deref(), Some("t7"), "the producer lands");
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.studio.current, Some("ghost".into()));
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.studio.current, Some("acme-shop".into()), "wraps");
    assert_eq!(app.focused.as_deref(), Some("t1"));
    // Focus alice, leave, come back → alice lands, not the producer.
    app.focused = Some("t4".into());
    app.refresh_projection();
    alt(&mut app, hc_vt::Key::ArrowDown);
    alt(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(app.studio.current, Some("acme-shop".into()));
    assert_eq!(
        app.focused.as_deref(),
        Some("t4"),
        "the remembered person lands"
    );
}

#[tokio::test]
async fn next_and_previous_role_skip_the_closed_and_wrap() {
    let mut app = two_studio_app();
    // acme cast order: producer, alice, (dave closed). From producer
    // → alice → wraps to producer; ← wraps back to alice.
    alt(&mut app, hc_vt::Key::ArrowRight);
    assert_eq!(app.focused.as_deref(), Some("t4"));
    alt(&mut app, hc_vt::Key::ArrowRight);
    assert_eq!(app.focused.as_deref(), Some("t1"), "dave skipped, wrapped");
    alt(&mut app, hc_vt::Key::ArrowLeft);
    assert_eq!(app.focused.as_deref(), Some("t4"), "wraps backwards");
}

#[tokio::test]
async fn a_role_key_from_outside_the_studio_falls_through() {
    let mut app = two_studio_app();
    app.focused = Some("t4".into());
    app.refresh_projection();
    assert_eq!(
        app.studio
            .remembered
            .get(&StudioKey::local("acme-shop"))
            .map(String::as_str),
        Some("actor-alice")
    );
    // Focus a terminal no person owns → not a studio workspace, so
    // the role key falls through to herdr's binding (unbound here:
    // nothing happens), and acme's remembered role stays put.
    app.focused = Some("t99".into());
    app.refresh_projection();
    alt(&mut app, hc_vt::Key::ArrowRight);
    assert_eq!(app.focused.as_deref(), Some("t99"), "herdr got the key");
    // Back inside the studio the same key cycles the cast — and the
    // remembered alice is where a studio hop lands.
    app.focused = Some("t1".into());
    app.refresh_projection();
    alt(&mut app, hc_vt::Key::ArrowRight);
    assert_eq!(app.focused.as_deref(), Some("t4"));
}

#[tokio::test]
async fn a_studio_with_no_live_terminal_still_becomes_current() {
    let mut app = two_studio_app();
    alt(&mut app, hc_vt::Key::ArrowDown);
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.studio.current, Some("ghost".into()));
    assert_eq!(
        app.focused.as_deref(),
        Some("t7"),
        "no focus move — nobody live to land on"
    );
    assert!(app.notice.is_some(), "the notice says so");
}

// ---------- routing ----------

/// herdr's config with `alt+left` bound to `previous_tab` — the usual
/// binding the studio keys shadow in studio mode.
fn live_with_alt_previous_tab() -> crate::config::LiveKeybindConfig {
    let config: crate::config::Config =
        toml::from_str("[keys]\nprevious_tab = 'alt+left'\n").unwrap();
    config.live_keybinds()
}

#[tokio::test]
async fn a_studio_key_wins_over_the_herdr_binding_on_the_same_key() {
    let mut app = two_studio_app();
    app.live = live_with_alt_previous_tab();
    // alt+left is herdr's previous_tab — but in studio mode it is
    // previous_role: alice lands, not another tab.
    alt(&mut app, hc_vt::Key::ArrowLeft);
    assert_eq!(app.focused.as_deref(), Some("t4"));
    assert_eq!(app.studio.current, Some("acme-shop".into()));
}

#[tokio::test]
async fn outside_studio_mode_the_herdr_binding_gets_the_key() {
    // Two tabs in one workspace; `previous_tab` cycles them.
    let mut app = app_with_tabs(&[("t1", "space-a", "tab-1"), ("t2", "space-a", "tab-2")]);
    app.live = live_with_alt_previous_tab();
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    assert!(!app.studio.active);
    alt(&mut app, hc_vt::Key::ArrowLeft);
    assert_eq!(
        app.focused.as_deref(),
        Some("t2"),
        "alt+left reached herdr's previous_tab"
    );
}

#[tokio::test]
async fn outside_studio_mode_an_unbound_studio_key_reaches_the_program() {
    let mut app = app_with(&["t1", "t2"]);
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    let before = app.pane.input_backlog().0;
    alt(&mut app, hc_vt::Key::ArrowRight);
    assert!(
        app.pane.input_backlog().0 > before,
        "the program got the key"
    );
}

#[tokio::test]
async fn a_locked_keyboard_gives_the_program_the_studio_keys() {
    let mut app = two_studio_app();
    app.keyboard_locked = true;
    let before = app.pane.input_backlog().0;
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.focused.as_deref(), Some("t1"), "no nav while locked");
    assert!(app.pane.input_backlog().0 > before, "the program got it");
}

#[tokio::test]
async fn the_studio_keys_fire_while_the_panel_owns_the_keys() {
    let mut app = two_studio_app();
    app.area = (120, 24);
    app.panel.mode = PanelMode::Expanded;
    app.panel.focused = true;
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.studio.current, Some("night-owl".into()));
    assert_eq!(app.focused.as_deref(), Some("t7"));
}

#[tokio::test]
async fn the_studio_keys_fire_while_a_persistent_float_is_focused() {
    let mut app = two_studio_app();
    app.area = (120, 24);
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    // A persistent float over the focused tab — spawned straight into
    // the shown state like the float tests do.
    app.run_custom_command(&crate::config::CustomCommandKeybind {
        bindings: Default::default(),
        label: "alt+f".into(),
        command: "exec \"${SHELL:-sh}\"".into(),
        action: crate::config::CustomCommandAction::Popup,
        description: None,
        width: Some(crate::popup_size::PopupSize::Percent(80)),
        height: Some(crate::popup_size::PopupSize::Percent(80)),
        persistent: true,
    });
    app.on_popup_spawned("t50".into(), 50);
    assert!(app.popup.is_some(), "the float shows");
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.studio.current, Some("night-owl".into()));
    assert_eq!(app.focused.as_deref(), Some("t7"), "the nav ran");
}

#[tokio::test]
async fn an_unbound_studio_key_does_nothing() {
    let mut app = two_studio_app();
    // `next_studio` unbound through the real config file.
    let dir = TestDir::new("studio-unbind");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "[keys]\nnext_studio = \"\"\n").unwrap();
    let (config, notice) = crate::panel::load(&path);
    assert!(notice.is_none());
    app.set_panel_config(config, notice, &path);
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(
        app.studio.current,
        Some("acme-shop".into()),
        "unbound: nothing ran"
    );
}

// ---------- geometry ----------

#[tokio::test]
async fn studio_and_role_switches_never_change_the_size_panes_ask_for() {
    let mut app = two_studio_app();
    app.area = (120, 24);
    // The size every pane asks the core for is `pane_inner` — the nav
    // only moves the focus, so each pane's own size must stay put
    // across a full studio+role tour.
    let terminals = ["t1", "t4", "t7", "t8"];
    let mut sizes = std::collections::BTreeMap::new();
    for terminal in terminals {
        app.focused = Some(terminal.into());
        app.refresh_projection();
        sizes.insert(terminal, pane_geometry::pane_inner(&mut app, 120, 24));
    }
    app.focused = Some("t1".into());
    app.refresh_projection();
    for key in [
        hc_vt::Key::ArrowDown,
        hc_vt::Key::ArrowUp,
        hc_vt::Key::ArrowLeft,
        hc_vt::Key::ArrowRight,
        hc_vt::Key::ArrowDown,
        hc_vt::Key::ArrowRight,
    ] {
        alt(&mut app, key);
    }
    for terminal in terminals {
        app.focused = Some(terminal.into());
        app.refresh_projection();
        assert_eq!(
            pane_geometry::pane_inner(&mut app, 120, 24),
            sizes[terminal],
            "nav resized {terminal}"
        );
    }
}

// Mouse hits for the cast rows, `others` and the blocks live in
// `studio_mouse`.

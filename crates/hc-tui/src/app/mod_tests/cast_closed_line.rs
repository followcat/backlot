//! App-level tests for the collapsed Cast closed-roles line: one target, click
//! and `Enter`/`o` opening the studio reopen picker, no stale-terminal attach,
//! and `--plain` / `[theme] source = "herdr"` invariance.
//!
//! Fixtures here are shared with `cast_reopen_picker`.

use std::sync::{Arc, Mutex};

use hc_proto::watch::{DocKey, Document};

use super::*;
use crate::app::input::{MouseButton, MouseEventKind, MouseInput, UiEvent};
use crate::focus::{CastTarget, WorkspaceRegion};
use crate::panel::PanelMode;
use crate::studio::StudioKey;

pub(super) fn doc_key() -> DocKey {
    DocKey {
        machine: None,
        name: "backlot".into(),
    }
}

pub(super) fn key(app: &mut App, k: hc_vt::Key) {
    app.on_input(
        UiEvent::Key(term_key(k, hc_vt::Mods::NONE, hc_vt::Action::Press)),
        app.area.0,
    );
}

pub(super) fn press_prefix(app: &mut App, ch: char) {
    app.on_input(
        UiEvent::Key(term_key(
            hc_vt::Key::Char('b'),
            hc_vt::Mods::CTRL,
            hc_vt::Action::Press,
        )),
        app.area.0,
    );
    key(app, hc_vt::Key::Char(ch));
}

pub(super) fn mouse(kind: MouseEventKind, col: u16, row: u16) -> MouseInput {
    MouseInput {
        kind,
        col,
        row,
        mods: hc_vt::Mods::NONE,
        sgr_position: None,
    }
}

pub(super) fn recorder() -> (
    Arc<Mutex<Vec<Vec<String>>>>,
    crate::app::action_tracker::TestRunner,
) {
    let recorded = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    let runner: crate::app::action_tracker::TestRunner = Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(String::new()) })
    });
    (recorded, runner)
}

pub(super) fn multi_closed_doc() -> serde_json::Value {
    let mut roles = vec![serde_json::json!({
        "role": "producer", "name": "producer", "terminal": "t1",
        "state": "live", "work": "working"
    })];
    // Intentional unsorted / string-order trap: t10, t2, t1, t16, ...
    let task_nums = [10u32, 2, 1, 16, 3, 4, 5, 6, 7, 8, 9, 11, 12, 13, 14, 15];
    let mut tasks = Vec::new();
    for (idx, n) in task_nums.iter().enumerate() {
        let name = if idx < 6 { "actor-a" } else { "actor-b" };
        let tid = format!("t{n}");
        roles.push(serde_json::json!({
            "role": format!("actor-t{n}"), "name": name, "terminal": format!("t9{n}"),
            "state": "closed", "resumable": true, "task": tid,
        }));
        tasks.push(serde_json::json!({
            "id": tid,
            "title": if *n == 1 {
                "Implement user authentication and long title clipping check".to_owned()
            } else { format!("Task number {n} title") },
            "status": "accepted", "priority": "normal", "requirement_version": 1,
            "rework_rounds": 0, "rework_cap": 3,
        }));
    }
    serde_json::json!({
        "schema": "backlot/1", "updated_ms": 1,
        "studios": [
            {
                "id": "acme-shop", "title": "Acme shop", "state": "live",
                "requirement_version": 1, "mode": "standard", "open_reviews": 0, "waiting": 0,
                "roles": roles, "tasks": tasks,
            },
            {
                "id": "beta-api", "title": "Beta API", "state": "live",
                "requirement_version": 1, "mode": "standard", "open_reviews": 0, "waiting": 0,
                "roles": [
                    { "role": "producer", "name": "producer", "terminal": "t7", "state": "live", "work": "waiting" },
                    { "role": "actor-other", "name": "alice", "state": "closed", "resumable": true, "task": "t1" }
                ],
                "tasks": [{ "id": "t1", "title": "Beta task", "status": "accepted", "priority": "normal", "requirement_version": 1, "rework_rounds": 0, "rework_cap": 3 }]
            }
        ]
    })
}

pub(super) fn closed_line_app() -> (App, Arc<Mutex<Vec<String>>>) {
    let mut app = app_with(&["t1", "t7"]);
    let attaches = Arc::new(Mutex::new(Vec::<String>::new()));
    let attaches_sink = attaches.clone();
    app.open_attach = Arc::new(move |params| {
        attaches_sink.lock().unwrap().push(params.terminal);
        Box::pin(std::future::pending())
    });
    app.pane = crate::app::pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
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
        doc_key(),
        Document {
            value: multi_closed_doc(),
            ..Document::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    app.panel.mode = PanelMode::Closed;
    app.area = (120, 36);
    (app, attaches)
}

#[tokio::test]
async fn cast_click_enter_and_o_open_numeric_studio_picker_without_attaching() {
    let (mut app, attaches) = closed_line_app();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    attaches.lock().unwrap().clear();

    // 1. Cast targets replace 16 closed role targets with ONE ClosedRoles target.
    let targets = app.cast_targets();
    assert_eq!(
        targets,
        vec![
            CastTarget::Role {
                studio: StudioKey::local("acme-shop"),
                role: "producer".into(),
            },
            CastTarget::ClosedRoles {
                studio: StudioKey::local("acme-shop"),
            },
        ]
    );

    // 2. Render Cast and verify stale closed terminals never register hits.
    draw_to(&mut app, 120, 36);
    assert!(
        app.hit_map.agents.iter().all(|(_, id)| id == "t1"),
        "closed stale terminals must not appear in hits.agents: {:?}",
        app.hit_map.agents
    );
    let (closed_rect, closed_studio) = app
        .hit_map
        .cast_closed
        .clone()
        .expect("closed line click rect");
    assert_eq!(closed_studio.id, "acme-shop");

    // 3. Left-click on closed line opens picker for acme-shop only and attaches nothing.
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        closed_rect.x + 2,
        closed_rect.y,
    ));
    assert!(
        attaches.lock().unwrap().is_empty(),
        "click must never attach"
    );
    assert!(app.help);
    let picker = app
        .help_state
        .picker
        .as_ref()
        .expect("reopen picker opened");
    assert_eq!(picker.title, "reopen a closed role");
    assert!(picker.standalone);
    assert_eq!(
        picker.rows.len(),
        16,
        "scoped strictly to acme-shop's 16 closed roles"
    );
    assert!(
        picker.rows[0].label.starts_with("t1   actor-a"),
        "row 0 format: {:?}",
        picker.rows[0].label
    );
    assert!(
        picker.rows[0]
            .title
            .as_deref()
            .is_some_and(|t| t.starts_with("Implement user authentication")),
        "row 0 title: {:?}",
        picker.rows[0].title
    );

    if std::env::var_os("CAST_CLOSED_SHOT").is_some() {
        let shot = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
        eprintln!("=== STANDALONE REOPEN PICKER ===\n{shot}");
    } else {
        draw_to(&mut app, 120, 36);
    }

    // 4. Esc closes picker completely back to Cast (not to launcher).
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.help);
    assert!(app.help_state.picker.is_none());
    assert_eq!(app.focus.workspace, WorkspaceRegion::Cast);

    // 5. Enter on focused ClosedRoles target opens the same picker without attaching.
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.help);
    assert_eq!(
        app.help_state.picker.as_ref().map(|p| p.title.as_str()),
        Some("reopen a closed role")
    );
    assert!(attaches.lock().unwrap().is_empty());

    // 6. Arrow down to `t2` and Enter -> runs `StudioAct::Reopen` for actor-t2.
    key(&mut app, hc_vt::Key::ArrowDown);
    key(&mut app, hc_vt::Key::Enter);
    assert!(!app.help);
    assert!(attaches.lock().unwrap().is_empty());
    {
        let argv = recorded.lock().unwrap();
        assert_eq!(argv.len(), 1, "{argv:?}");
        assert_eq!(
            argv[0],
            vec![
                "reopen",
                "--person",
                "actor-t2",
                "--studio",
                "acme-shop",
                "--as",
                "user"
            ]
        );
    }

    // 7. Pressing `o` directly on focused ClosedRoles target opens the same picker.
    press_prefix(&mut app, 'a');
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(
        app.cast_focus,
        Some(CastTarget::ClosedRoles {
            studio: StudioKey::local("acme-shop")
        })
    );
    key(&mut app, hc_vt::Key::Char('o'));
    assert!(app.help);
    assert_eq!(
        app.help_state.picker.as_ref().map(|p| p.title.as_str()),
        Some("reopen a closed role")
    );
}

#[tokio::test]
async fn plain_mode_and_source_herdr_stay_unchanged_without_stale_attach() {
    // 1. Plain mode unchanged: agents sidebar (no cast closed line).
    let (mut plain_app, plain_attaches) = closed_line_app();
    plain_app.set_plain(true);
    let plain_text = screen_lines(&draw_to(&mut plain_app, 120, 30)).join("\n");
    assert!(!plain_text.contains("──╡ ✦ CAST ╞"), "{plain_text}");
    assert!(!plain_text.contains("closed · actor-"), "{plain_text}");
    assert!(plain_attaches.lock().unwrap().is_empty());

    // 2. `[theme] source = "herdr"` (`!plain && !studio_chrome`): keeps `┄ closed (n) ┄`
    // and individual closed rows while NEVER putting closed roles' stale terminal in hits.agents.
    let (mut herdr_app, herdr_attaches) = closed_line_app();
    let dir = crate::test_support::TestDir::new("cast-closed-herdr");
    std::fs::create_dir_all(&dir).unwrap();
    let own = dir.join("own.toml");
    std::fs::write(&own, "[theme]\nsource = \"herdr\"\n").unwrap();
    herdr_app.shell.own_theme = crate::own_theme::Shell::at(own);
    herdr_app.reload_config();
    assert!(!herdr_app.shell.studio_chrome);

    let text = screen_lines(&draw_to(&mut herdr_app, 120, 36)).join("\n");
    assert!(
        text.contains("┄ closed (16) ┄"),
        "source=herdr keeps old divider:\n{text}"
    );
    assert!(
        herdr_app.hit_map.cast_closed.is_none(),
        "source=herdr keeps old Cast behavior"
    );
    assert!(
        herdr_app
            .hit_map
            .agents
            .iter()
            .all(|(_, id)| !id.starts_with("t9")),
        "closed stale terminals still excluded from hits.agents: {:?}",
        herdr_app.hit_map.agents
    );
    // Clicking a closed role row in source=herdr attaches nothing.
    let closed_row_y = text
        .lines()
        .position(|l| l.contains("✓") && l.contains("t10"))
        .expect("old closed row under divider in source=herdr") as u16;
    herdr_app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        4,
        closed_row_y,
    ));
    assert!(
        herdr_attaches.lock().unwrap().is_empty(),
        "no attach on closed role in source=herdr"
    );
}

//! W1's left side: every machine's studios from the ledger, the
//! finished group, the tree that scrolls with the selection, the cast
//! that explains itself and the stage log that keeps the focus and
//! drops keys. Neutral fixtures only.

use std::sync::Arc;
use std::time::Instant;

use super::studio_machines::two_machine_studio_app;
use super::*;
use crate::app::input::MouseButton;
use crate::app::input::MouseEventKind;
use crate::chrome::render::StudioRowAct;
use crate::panel::StudioNav;
use crate::sidebar::fixture::{machine, term};
use crate::studio_menu::StudioAct;
use hc_proto::watch::{DocKey, Document};
use hc_vt::{Action, Key, Mods};
use serde_json::json;

const HOST_B_LIST: &str = r#"{
    "pinned": true,
    "studios": [{"name": "beta-tools", "state": "live", "last_ms": 3}]
}"#;

fn doc_key(machine: Option<&str>) -> DocKey {
    DocKey {
        machine: machine.map(str::to_owned),
        name: "backlot".into(),
    }
}

fn sidebar_text(b: &TestBackend) -> String {
    let (h, w) = (b.size().unwrap().height, 26u16);
    let buf = b.buffer();
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn click(app: &mut App, rect: ratatui::layout::Rect) {
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        app.on_mouse(crate::app::input::MouseInput {
            kind,
            col: rect.x + 1,
            row: rect.y,
            mods: Mods::NONE,
            sgr_position: None,
        });
    }
}

/// `two_machine_studio_app` with host-b's stage gone and its document
/// unpublished — the ledger alone knows `beta-tools`.
pub(super) fn stage_down_host_b() -> App {
    let mut app = two_machine_studio_app();
    let version = app.view.version + 1;
    let terms: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .filter(|t| !t.address.starts_with("host-b/"))
        .cloned()
        .collect();
    app.view.inventory_mut().replace(version, terms);
    app.view.documents_mut().remove(&doc_key(Some("host-b")));
    app.studio_index.note_reply(
        Some("host-b".into()),
        false,
        Instant::now(),
        Ok(HOST_B_LIST),
    );
    app.reconcile_published_view();
    app
}

#[tokio::test]
async fn a_stage_down_machine_lists_its_studios_from_the_ledger() {
    let mut app = stage_down_host_b();
    app.area = (120, 28);
    let backend = draw_to(&mut app, 120, 28);
    let sidebar = sidebar_text(&backend);
    // W1b: the name first and the shortened `down` tag keep a
    // 10-character id whole at the default 26 columns.
    assert!(sidebar.contains("beta-tools"), "whole id: {sidebar}");
    assert!(sidebar.contains(" down"), "short tag: {sidebar}");
    let start = app
        .hit_map
        .studio_rows
        .iter()
        .find(|hit| hit.act == StudioRowAct::StartStage)
        .expect("the stage-down studio row")
        .rect;
    // Enter / a click is the row's `s`: it starts that machine's stage.
    click(&mut app, start);
    assert_eq!(
        app.actions.pending.len(),
        1,
        "the press starts host-b's stage"
    );
    assert_eq!(app.actions.pending[0].action.argv, ["stage", "start"]);
    assert_eq!(
        app.actions.pending[0].action.machine.as_deref(),
        Some("host-b")
    );
}

#[tokio::test]
async fn an_unreachable_machine_is_one_line_not_an_empty_tree() {
    let mut app = stage_down_host_b();
    app.view.machines = Some(vec![machine("host-b", "unreachable")]);
    app.reconcile_published_view();
    app.area = (120, 28);
    let backend = draw_to(&mut app, 120, 28);
    let sidebar = sidebar_text(&backend);
    assert_eq!(sidebar.matches("unreachable").count(), 1, "{sidebar}");
    assert!(!sidebar.contains("beta-tools"), "{sidebar}");
}

fn with_finished_local_studio() -> App {
    let mut app = two_machine_studio_app();
    let key = doc_key(None);
    let mut value = app.view.documents.get(&key).unwrap().value.clone();
    value["studios"].as_array_mut().unwrap().push(json!({
        "id": "old-shop", "title": "Old shop", "state": "finished",
        "requirement_version": 1, "mode": "standard",
        "open_reviews": 0, "waiting": 0, "roles": []
    }));
    app.view.documents_mut().insert(
        key,
        Arc::new(Document {
            value,
            ..Document::default()
        }),
    );
    app.reconcile_published_view();
    app
}

#[tokio::test]
async fn the_finished_group_counts_unfolds_and_opens_the_finished_menu() {
    let mut app = with_finished_local_studio();
    app.area = (120, 32);
    let backend = draw_to(&mut app, 120, 32);
    let sidebar = sidebar_text(&backend);
    assert!(sidebar.contains("finished (1)"), "{sidebar}");
    assert!(!sidebar.contains("old-shop"), "folded: {sidebar}");

    let fold = app
        .hit_map
        .studio_rows
        .iter()
        .find(|hit| hit.act == StudioRowAct::ToggleFinished)
        .expect("the finished fold row")
        .rect;
    click(&mut app, fold);
    assert!(app.studio.finished_open.contains(&None));
    let backend = draw_to(&mut app, 120, 32);
    let sidebar = sidebar_text(&backend);
    assert!(sidebar.contains("old-shop"), "unfolded: {sidebar}");

    let studio = app
        .hit_map
        .studio_rows
        .iter()
        .find(|hit| matches!(hit.act, StudioRowAct::OpenFinished { .. }))
        .expect("the finished studio row")
        .rect;
    click(&mut app, studio);
    let Some(menu) = &app.menu else {
        panic!("the finished studio menu opened");
    };
    let crate::chrome::menu::ContextMenuTarget::Studio(menu) = &menu.target else {
        panic!("a studio menu — {:?}", menu.target);
    };
    assert!(menu.finished, "the finished list");
    let labels: Vec<String> = menu.items().into_iter().map(|item| item.label).collect();
    assert!(
        labels.iter().any(|l| l == "Reopen producer (o)"),
        "{labels:?}"
    );
    assert!(labels.iter().any(|l| l.contains("sheet")), "{labels:?}");
    assert!(
        labels.iter().any(|l| l.contains("requirement")),
        "{labels:?}"
    );
    assert!(labels.iter().any(|l| l.contains("records")), "{labels:?}");
    assert!(
        labels.iter().any(|l| l.contains("Delete studio")),
        "{labels:?}"
    );
}

#[tokio::test]
async fn the_machine_menu_folds_and_unfolds_the_finished_group() {
    let mut app = with_finished_local_studio();
    let menu = app.machine_menu(&crate::chrome::snapshot::ClientEndpointId::Local);
    let unfold = menu
        .items()
        .into_iter()
        .find(|item| item.act == StudioAct::ToggleFinishedGroup)
        .expect("the finished fold row");
    assert_eq!(unfold.label, "Unfold finished studios (1)");
    app.run_studio_act(&menu, unfold.act);
    assert!(app.studio.finished_open.contains(&None), "unfolded");
    let menu = app.machine_menu(&crate::chrome::snapshot::ClientEndpointId::Local);
    let fold = menu
        .items()
        .into_iter()
        .find(|item| item.act == StudioAct::ToggleFinishedGroup)
        .expect("the fold row again");
    assert_eq!(fold.label, "Fold finished studios (1)");
}

#[tokio::test]
async fn a_short_sidebar_scrolls_to_the_selected_machines_row() {
    let mut app = two_machine_studio_app();
    app.area = (120, 20);
    // Give the machine rows enough workspaces that the section scrolls.
    app.shell_state.workspace_scroll = 0;
    let first = draw_to(&mut app, 120, 20);
    assert!(sidebar_text(&first).contains("acme-shop"), "fixture");
    // Alt+right lands on host-b's `beta-tools`.
    app.studio_nav(StudioNav::NextStudio);
    assert_eq!(
        app.studio.current.as_ref().map(|key| key.id.as_str()),
        Some("beta-tools")
    );
    let backend = draw_to(&mut app, 120, 20);
    let sidebar = sidebar_text(&backend);
    assert!(
        sidebar.contains("host-b"),
        "the machine row stays: {sidebar}"
    );
    assert!(sidebar.contains("beta-tools"), "the studio row: {sidebar}");
}

/// One machine's studios with no current studio: the build must show a
/// lone live studio and hint when more than one exists.
#[test]
fn the_cast_shows_the_only_live_studio_and_hints_otherwise() {
    let one: backlot_model::summary::Summary = serde_json::from_value(json!({
        "schema": "backlot/1", "updated_ms": 1,
        "studios": [{"id": "acme-shop", "title": "Acme", "state": "live",
                     "requirement_version": 1, "mode": "standard",
                     "open_reviews": 0, "waiting": 0,
                     "roles": [{"role": "producer", "name": "producer", "state": "closed"}]}]
    }))
    .unwrap();
    let machines = vec![crate::panel::data::MachineSummary {
        machine: None,
        summary: one,
    }];
    let view = hc_proto::watch::View::default();
    let config = crate::chrome::ClientShellConfig::from_config(
        &crate::config::Config::default(),
        None,
        None,
        std::path::PathBuf::new(),
    );
    let snapshot = crate::chrome::snapshot::ClientShellSnapshot {
        boot_id: String::new(),
        revision: 0,
        focused_workspace_id: None,
        focused_tab_id: None,
        focused_pane_id: None,
        tab_bar_right: Vec::new(),
        tab_bar_right_separator: String::new(),
        agent_order: Vec::new(),
        agent_view_label: None,
        workspaces: Vec::new(),
        tabs: Vec::new(),
        panes: Vec::new(),
        agents: Vec::new(),
        commands: Vec::new(),
    };
    let cast = crate::cast::CastView::build(
        &machines,
        &snapshot,
        &view,
        None,
        Some(crate::project::LOCAL_LABEL),
        &config,
        false,
        None,
        0,
    );
    assert!(cast.studio.is_some(), "the only live studio shows");
    assert!(!cast.hint);

    let two: backlot_model::summary::Summary = serde_json::from_value(json!({
        "schema": "backlot/1", "updated_ms": 1,
        "studios": [
            {"id": "acme-shop", "title": "Acme", "state": "live",
             "requirement_version": 1, "mode": "standard",
             "open_reviews": 0, "waiting": 0, "roles": []},
            {"id": "beta-tools", "title": "Beta", "state": "live",
             "requirement_version": 1, "mode": "standard",
             "open_reviews": 0, "waiting": 0, "roles": []}
        ]
    }))
    .unwrap();
    let machines = vec![crate::panel::data::MachineSummary {
        machine: None,
        summary: two,
    }];
    let cast = crate::cast::CastView::build(
        &machines,
        &snapshot,
        &view,
        None,
        Some(crate::project::LOCAL_LABEL),
        &config,
        false,
        None,
        0,
    );
    assert!(cast.studio.is_none(), "two studios need a choice");
    assert!(cast.hint, "the hint answers for them");
}

fn stage_app() -> App {
    let mut stage = term("t9");
    stage.name = Some("backlot-stage".into());
    stage.labels.insert("backlot.role".into(), "stage".into());
    stage.labels.insert("herdr.space".into(), "backlot".into());
    stage.labels.insert("herdr.tab".into(), "stage".into());
    let mut person = term("t1");
    person
        .labels
        .insert("herdr.space".into(), "acme-shop".into());
    person.labels.insert("herdr.tab".into(), "main".into());
    let mut app = app_with_terms(vec![person, stage]);
    app.set_plain(false);
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    app
}

#[tokio::test]
async fn a_stage_start_keeps_the_focus_off_the_stage_tab() {
    let mut app = stage_app();
    app.note_stage_start();
    // The core focuses the new stage terminal in the shared layout.
    let key = crate::project::layout_key(
        None,
        app.projection.snapshot.focused_tab_id.as_ref().unwrap(),
    );
    let value = json!({
        "schema": 1,
        "focus": "t9",
        "tabs": {key: {"root": {"type": "pane", "terminal": "t9"}}}
    });
    app.view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "hc-tui".into(),
        },
        Arc::new(Document {
            value,
            ..Document::default()
        }),
    );
    app.reconcile_published_view();
    assert_eq!(
        app.focused.as_deref(),
        Some("t1"),
        "the stage log never takes focus"
    );
}

#[tokio::test]
async fn keys_to_the_stage_terminal_are_dropped_and_the_bar_says_read_only() {
    let mut app = stage_app();
    app.focused = Some("t9".into());
    app.refresh_projection();
    app.sync_target();
    assert_eq!(app.pane.target(), Some("t9"));
    app.on_key(term_key(Key::Char('j'), Mods::NONE, Action::Press), 120);
    assert!(
        app.pane_leases.forwarded().next().is_none(),
        "no press reached the stage process"
    );
    app.on_key(term_key(Key::Char('j'), Mods::NONE, Action::Release), 120);
    app.area = (120, 24);
    let backend = draw_to(&mut app, 120, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("stage log — read only"), "{text}");
}

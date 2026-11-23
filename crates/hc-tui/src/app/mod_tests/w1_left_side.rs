//! W1's left side: every machine's studios from the ledger, the
//! finished group and the tree that scrolls with the selection.
//! Neutral fixtures only.

use std::time::Instant;

use super::studio_machines::two_machine_studio_app;
use super::*;
use crate::app::input::MouseButton;
use crate::app::input::MouseEventKind;
use crate::chrome::render::StudioRowAct;
use crate::panel::StudioNav;
use crate::sidebar::fixture::machine;
use crate::studio_menu::StudioAct;
use hc_proto::watch::{DocKey, Document};
use hc_vt::Mods;
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
fn stage_down_host_b() -> App {
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
    // 26 columns with the scrollbar: the id keeps its head, the tag
    // keeps its cells.
    assert!(sidebar.contains("beta-too"), "{sidebar}");
    assert!(sidebar.contains("stage down"), "{sidebar}");
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
    // Alt+down lands on host-b's `beta-tools`.
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

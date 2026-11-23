//! W1b's launcher `machines` group: the stage log, the finished fold
//! and the machine menu join the stage rows, and each runs its own
//! action through `run_studio_act` / `open_studio_menu`. Neutral
//! fixtures only (`host-b`, `acme-shop`, `beta-tools`).

use super::studio_machines::two_machine_studio_app;
use super::*;
use crate::help_launcher::{HelpRow, Launch};
use crate::studio_menu::StudioAct;
use hc_proto::watch::{DocKey, Document};

/// Every row of the `machines` group, flattened.
fn machine_rows(app: &App) -> Vec<HelpRow> {
    app.help_groups()
        .into_iter()
        .filter(|group| group.title == "machines")
        .flat_map(|group| group.rows)
        .collect()
}

fn labels(app: &App) -> Vec<String> {
    machine_rows(app).into_iter().map(|row| row.label).collect()
}

fn row_for(app: &App, label: &str) -> HelpRow {
    machine_rows(app)
        .into_iter()
        .find(|row| row.label == label)
        .unwrap_or_else(|| panic!("no `{label}` row — {:?}", labels(app)))
}

/// host-b's document names the running `backlot`, so its machine menu
/// offers the stage rows — what a running stage publishes.
fn installed_app() -> App {
    let mut app = two_machine_studio_app();
    let key = DocKey {
        machine: Some("host-b".into()),
        name: "backlot".into(),
    };
    let doc = app.view.documents_mut().get_mut(&key).expect("host-b doc");
    std::sync::Arc::make_mut(doc).value["backlot_version"] =
        serde_json::Value::String(env!("CARGO_PKG_VERSION").into());
    app.reconcile_published_view();
    app
}

#[tokio::test]
async fn the_machines_group_offers_the_stage_log_and_menu_of_a_running_stage() {
    let app = installed_app();
    let listed = labels(&app);
    assert!(
        listed.contains(&"Stage log · host-b".to_owned()),
        "{listed:?}"
    );
    assert!(
        listed.contains(&"Machine menu · host-b".to_owned()),
        "{listed:?}"
    );
    let log = row_for(&app, "Stage log · host-b");
    let Launch::Act { act, .. } = log.launch else {
        panic!("the log is an act — {:?}", log.launch);
    };
    assert_eq!(act, StudioAct::StageLog);
    let menu = row_for(&app, "Machine menu · host-b");
    assert!(
        matches!(menu.launch, Launch::Menu(_)),
        "the menu row carries its menu — {:?}",
        menu.launch
    );
}

#[tokio::test]
async fn the_stage_log_row_opens_the_stages_read_only_peek() {
    let mut app = installed_app();
    let row = row_for(&app, "Stage log · host-b");
    let Launch::Act { menu, act } = row.launch else {
        panic!("an act row");
    };
    app.run_studio_act(&menu, act);
    let peek = app.peek.as_ref().expect("the stage log opened");
    assert!(
        peek.terminal.starts_with("host-b/"),
        "the peek is host-b's stage — {}",
        peek.terminal
    );
}

#[tokio::test]
async fn the_finished_fold_row_toggles_that_machines_group() {
    let mut app = installed_app();
    push_finished_local_studio(&mut app);
    let listed = labels(&app);
    assert!(
        listed.contains(&"Unfold finished studios (1) · Local".to_owned()),
        "{listed:?}"
    );
    let row = row_for(&app, "Unfold finished studios (1) · Local");
    let Launch::Act { menu, act } = row.launch else {
        panic!("an act row");
    };
    assert_eq!(act, StudioAct::ToggleFinishedGroup);
    app.run_studio_act(&menu, act);
    assert!(app.studio.finished_open.contains(&None), "unfolded");
    let folded = labels(&app);
    assert!(
        folded.contains(&"Fold finished studios (1) · Local".to_owned()),
        "{folded:?}"
    );
}

#[tokio::test]
async fn the_machine_menu_row_opens_the_right_click_menu() {
    let mut app = installed_app();
    app.area = (120, 32);
    draw_to(&mut app, 120, 32);
    let row = row_for(&app, "Machine menu · host-b");
    let Launch::Menu(menu) = row.launch else {
        panic!("a menu row");
    };
    app.open_studio_menu(*menu, (4, 4));
    let Some(opened) = &app.menu else {
        panic!("the menu opened");
    };
    let crate::chrome::menu::ContextMenuTarget::Studio(menu) = &opened.target else {
        panic!("a studio menu — {:?}", opened.target);
    };
    assert_eq!(
        menu.kind,
        crate::studio_menu::MenuKind::Machine,
        "the same machine menu a right-click opens"
    );
    assert_eq!(menu.machine.as_deref(), Some("host-b"));
}

#[tokio::test]
async fn a_machine_behind_latest_still_lists_its_menu() {
    let mut app = installed_app();
    // A newer release than this crate's is offered: the upgrade row
    // leads, then the stage's own rows and the menu.
    app.update_latest = Some("99.0.0".to_owned());
    let listed = labels(&app);
    assert!(
        listed.iter().any(|l| l.starts_with("Upgrade backlot to v")),
        "{listed:?}"
    );
    assert!(
        listed.contains(&"Machine menu · host-b".to_owned()),
        "{listed:?}"
    );
}

/// A finished `old-shop` in this core's document — the fold's count.
fn push_finished_local_studio(app: &mut App) {
    let key = DocKey {
        machine: None,
        name: "backlot".into(),
    };
    let mut value = app.view.documents.get(&key).unwrap().value.clone();
    value["studios"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": "old-shop", "title": "Old shop", "state": "finished",
            "requirement_version": 1, "mode": "standard",
            "open_reviews": 0, "waiting": 0, "roles": []
        }));
    app.view.documents_mut().insert(
        key,
        Document {
            value,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
}

// ---------- the tree's keyboard path (Ctrl+B t) ----------

use super::w1_left_side::stage_down_host_b;
use crate::chrome::studio_tree::SidebarRow;

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

fn key(app: &mut App, k: hc_vt::Key, mods: hc_vt::Mods) {
    app.on_input(
        UiEvent::Key(term_key(k, mods, hc_vt::Action::Press)),
        app.area.0,
    );
}

#[tokio::test]
async fn the_focus_chord_enters_the_tree_and_esc_leaves() {
    let mut app = two_machine_studio_app();
    app.area = (120, 28);
    draw_to(&mut app, 120, 28);
    assert!(!app.tree_focused(), "the tree does not own the keys yet");
    press_tree_chord(&mut app);
    assert!(app.tree_focused(), "Ctrl+B t hands the tree the keys");
    assert!(
        app.tree_focus.is_some(),
        "the cursor lands on a row — {:?}",
        app.tree_focus
    );
    key(&mut app, hc_vt::Key::Escape, hc_vt::Mods::NONE);
    assert!(!app.tree_focused(), "Esc returns the keys");
}

#[tokio::test]
async fn the_focus_chord_opens_a_collapsed_sidebar() {
    let mut app = two_machine_studio_app();
    app.area = (120, 28);
    app.shell_state.sidebar_collapsed = true;
    press_tree_chord(&mut app);
    assert!(!app.shell_state.sidebar_collapsed, "the tree is visible");
    assert!(app.tree_focused());
}

#[tokio::test]
async fn arrows_move_the_cursor_to_the_next_row() {
    let mut app = two_machine_studio_app();
    app.area = (120, 28);
    draw_to(&mut app, 120, 28);
    press_tree_chord(&mut app);
    let start = app.tree_focus.clone().expect("a start row");
    key(&mut app, hc_vt::Key::ArrowDown, hc_vt::Mods::NONE);
    assert_ne!(app.tree_focus, Some(start), "↓ moves to another row");
    let moved = app.tree_focus.clone().unwrap();
    key(&mut app, hc_vt::Key::ArrowUp, hc_vt::Mods::NONE);
    assert_ne!(app.tree_focus, Some(moved), "↑ moves back off it");
}

#[tokio::test]
async fn enter_on_a_machine_row_folds_its_section() {
    let mut app = two_machine_studio_app();
    app.area = (120, 28);
    draw_to(&mut app, 120, 28);
    press_tree_chord(&mut app);
    // Land the cursor on a machine row.
    let rows = app.tree_rows();
    let machine = rows
        .iter()
        .find(|row| matches!(row, SidebarRow::Machine(_)))
        .cloned()
        .expect("a machine row");
    app.tree_focus = Some(machine.clone());
    let SidebarRow::Machine(endpoint) = machine else {
        unreachable!()
    };
    assert!(!app.shell_state.collapsed_endpoints.contains(&endpoint));
    key(&mut app, hc_vt::Key::Enter, hc_vt::Mods::NONE);
    assert!(
        app.shell_state.collapsed_endpoints.contains(&endpoint),
        "Enter folds the machine's section"
    );
}

#[tokio::test]
async fn enter_on_a_stage_down_studio_starts_the_stage() {
    let mut app = stage_down_host_b();
    app.area = (120, 28);
    draw_to(&mut app, 120, 28);
    press_tree_chord(&mut app);
    let row = app
        .tree_rows()
        .into_iter()
        .find(|row| matches!(row, SidebarRow::Studio { row: crate::chrome::studio_tree::Row::Studio { id, .. }, .. } if id == "beta-tools"))
        .expect("the stage-down studio row");
    app.tree_focus = Some(row);
    key(&mut app, hc_vt::Key::Enter, hc_vt::Mods::NONE);
    assert_eq!(app.actions.pending.len(), 1, "Enter starts the stage");
    assert_eq!(app.actions.pending[0].action.argv, ["stage", "start"]);
    assert_eq!(
        app.actions.pending[0].action.machine.as_deref(),
        Some("host-b")
    );
}

#[tokio::test]
async fn enter_on_the_finished_fold_toggles_it() {
    let mut app = installed_app();
    push_finished_local_studio(&mut app);
    app.area = (120, 32);
    draw_to(&mut app, 120, 32);
    press_tree_chord(&mut app);
    let row = app
        .tree_rows()
        .into_iter()
        .find(|row| {
            matches!(
                row,
                SidebarRow::Studio {
                    row: crate::chrome::studio_tree::Row::FinishedFold { .. },
                    ..
                }
            )
        })
        .expect("the finished fold row");
    app.tree_focus = Some(row);
    key(&mut app, hc_vt::Key::Enter, hc_vt::Mods::NONE);
    assert!(app.studio.finished_open.contains(&None), "the fold opened");
}

#[tokio::test]
async fn enter_on_a_finished_studio_opens_its_menu() {
    let mut app = installed_app();
    push_finished_local_studio(&mut app);
    app.studio.finished_open.insert(None);
    app.area = (120, 32);
    draw_to(&mut app, 120, 32);
    press_tree_chord(&mut app);
    let row = app
        .tree_rows()
        .into_iter()
        .find(|row| matches!(row, SidebarRow::Studio { row: crate::chrome::studio_tree::Row::Finished { id }, .. } if id == "old-shop"))
        .expect("the finished studio row");
    app.tree_focus = Some(row);
    key(&mut app, hc_vt::Key::Enter, hc_vt::Mods::NONE);
    let Some(menu) = &app.menu else {
        panic!("the finished studio menu opened");
    };
    let crate::chrome::menu::ContextMenuTarget::Studio(menu) = &menu.target else {
        panic!("a studio menu — {:?}", menu.target);
    };
    assert!(menu.finished, "the finished-studio list");
}

#[tokio::test]
async fn m_on_a_machine_row_opens_its_menu() {
    let mut app = installed_app();
    app.area = (120, 32);
    draw_to(&mut app, 120, 32);
    press_tree_chord(&mut app);
    let rows = app.tree_rows();
    let machine = rows
        .iter()
        .find(|row| matches!(row, SidebarRow::Machine(_)))
        .cloned()
        .expect("a machine row");
    app.tree_focus = Some(machine);
    key(&mut app, hc_vt::Key::Char('m'), hc_vt::Mods::NONE);
    let Some(menu) = &app.menu else {
        panic!("the machine menu opened");
    };
    let crate::chrome::menu::ContextMenuTarget::Studio(menu) = &menu.target else {
        panic!("a studio menu — {:?}", menu.target);
    };
    assert_eq!(menu.kind, crate::studio_menu::MenuKind::Machine);
}

#[tokio::test]
async fn the_tree_cursor_paints_the_highlighted_row() {
    let mut app = two_machine_studio_app();
    app.area = (120, 28);
    draw_to(&mut app, 120, 28);
    press_tree_chord(&mut app);
    let backend = draw_to(&mut app, 120, 28);
    let buf = backend.buffer();
    // The cursor's `▌` sits on the highlighted row's left column.
    let highlighted = (0..28).any(|y| buf[(0, y)].symbol() == "▌");
    assert!(highlighted, "the tree row carries the accent cursor");
}

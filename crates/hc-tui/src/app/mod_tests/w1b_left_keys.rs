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

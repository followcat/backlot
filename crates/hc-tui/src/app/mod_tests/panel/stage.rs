//! The Dock's stopped-stage rows (R3 §1b): the block's owed-step
//! line and the bare machine row both say `stage stopped · s start`,
//! a bare `s` runs `backlot stage start` on the row's machine and
//! Enter on the `Stage` row matches. Neutral fixtures only
//! (`acme-shop`, `beta-tools`, `host-b`).

use super::*;
use crate::studio::StudioKey;

fn recorder() -> (
    std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    crate::app::action_tracker::TestRunner,
) {
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    let runner: crate::app::action_tracker::TestRunner = std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(String::new()) })
    });
    (recorded, runner)
}

/// `address`'s terminal leaves the inventory — the machine's stage
/// reads down while its document stands.
fn drop_terminal(app: &mut App, address: &str) {
    let terms: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .filter(|t| t.address != address)
        .cloned()
        .collect();
    app.view.inventory_mut().replace(2, terms);
}

/// The pending `stage start`, or `None` — its `machine` is the
/// routing the remote runner would see.
fn stage_start_pending(app: &App) -> Option<Option<String>> {
    app.actions
        .pending
        .iter()
        .find(|p| p.action.argv == ["stage", "start"])
        .map(|p| p.action.machine.clone())
}

/// Select the first `Target::Stage` the dock currently draws —
/// `expect` names the machine it wanted.
fn select_stage_row(app: &mut App, machine: Option<&str>) {
    let want = Target::Stage(machine.map(str::to_owned));
    assert!(
        app.panel_targets().contains(&want),
        "a stage row for {machine:?} — {:?}",
        app.panel_targets()
    );
    app.panel.selected = Some(want);
}

#[tokio::test]
async fn the_studio_block_says_stage_stopped_and_s_starts_it() {
    // Local acme-shop, its stage terminal gone — the block's owed
    // line carries `◌ stage stopped · s start`.
    let mut app = app_with_panel_doc();
    let (_recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (160, 24);
    drop_terminal(&mut app, "t9");
    open_panel(&mut app);
    let text = screen_lines(&draw_to(&mut app, 160, 24)).join("\n");
    assert!(text.contains("stage stopped · s start"), "{text}");
    // `s` on the owed line itself starts the local stage.
    select_stage_row(&mut app, None);
    key(&mut app, hc_vt::Key::Char('s'));
    assert_eq!(stage_start_pending(&app), Some(None));
}

#[tokio::test]
async fn s_on_a_studio_row_starts_its_machines_stage() {
    // host-b's `beta-tools` block with host-b's stage down: `s` on a
    // studio row inside the block routes `stage start` to host-b.
    let mut app = app_with_two_machines();
    let (_recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (160, 24);
    drop_terminal(&mut app, "host-b/t9");
    open_panel(&mut app);
    app.panel.selected = Some(Target::Studio(StudioKey {
        machine: Some("host-b".into()),
        id: "beta-tools".into(),
    }));
    key(&mut app, hc_vt::Key::Char('s'));
    assert_eq!(stage_start_pending(&app), Some(Some("host-b".into())));

    // And `s` on the expanded block's stage row is the same dispatch.
    let mut app = app_with_two_machines();
    let (_recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (160, 24);
    drop_terminal(&mut app, "host-b/t9");
    open_panel(&mut app);
    app.panel.expanded_blocks.insert(StudioKey {
        machine: Some("host-b".into()),
        id: "beta-tools".into(),
    });
    app.panel.selected = Some(Target::Studio(StudioKey {
        machine: Some("host-b".into()),
        id: "beta-tools".into(),
    }));
    select_stage_row(&mut app, Some("host-b"));
    key(&mut app, hc_vt::Key::Char('s'));
    assert_eq!(stage_start_pending(&app), Some(Some("host-b".into())));
}

#[tokio::test]
async fn enter_on_the_stage_row_starts_it() {
    // Enter on `Target::Stage` matches `s` — local and remote.
    let mut app = app_with_two_machines();
    let (_recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (160, 24);
    drop_terminal(&mut app, "host-b/t9");
    open_panel(&mut app);
    app.panel.expanded_blocks.insert(StudioKey {
        machine: Some("host-b".into()),
        id: "beta-tools".into(),
    });
    app.panel.selected = Some(Target::Studio(StudioKey {
        machine: Some("host-b".into()),
        id: "beta-tools".into(),
    }));
    select_stage_row(&mut app, Some("host-b"));
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(stage_start_pending(&app), Some(Some("host-b".into())));
}

#[tokio::test]
async fn a_bare_machine_row_says_stage_stopped_and_s_starts_it() {
    // host-b's document answers with no studios and no stage — the
    // dock's own line names the machine. host-c's studio keeps the
    // aggregate spanning machines, so the name prints.
    let mut app = app_with_two_machines();
    let (_recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (160, 24);
    app.view
        .machines
        .as_mut()
        .unwrap()
        .push(crate::sidebar::fixture::machine("host-c", "connected"));
    app.view.documents_mut().insert(
        DocKey {
            machine: Some("host-c".into()),
            name: "backlot".into(),
        },
        Document {
            value: remote_summary_json(),
            ..Document::default()
        }
        .into(),
    );
    let mut stage = crate::sidebar::fixture::term("host-c/t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    let mut terms: Vec<_> = app.view.inventory.terminals().values().cloned().collect();
    terms.push(stage);
    app.view.inventory_mut().replace(3, terms);
    let doc = app
        .view
        .documents_mut()
        .get_mut(&DocKey {
            machine: Some("host-b".into()),
            name: "backlot".into(),
        })
        .expect("host-b's document");
    std::sync::Arc::make_mut(doc).value["studios"] = serde_json::json!([]);
    drop_terminal(&mut app, "host-b/t9");
    app.reconcile_published_view();
    open_panel(&mut app);
    let text = screen_lines(&draw_to(&mut app, 160, 24)).join("\n");
    assert!(text.contains("host-b · stage stopped · s start"), "{text}");
    select_stage_row(&mut app, Some("host-b"));
    key(&mut app, hc_vt::Key::Char('s'));
    assert_eq!(stage_start_pending(&app), Some(Some("host-b".into())));
}

#[tokio::test]
async fn s_is_a_no_op_while_the_stage_runs() {
    let mut app = app_with_panel_doc();
    let (_recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (160, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Studio(StudioKey {
        machine: None,
        id: "acme-shop".into(),
    }));
    key(&mut app, hc_vt::Key::Char('s'));
    assert_eq!(stage_start_pending(&app), None);
}

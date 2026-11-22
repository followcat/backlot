//! The Call Sheet's stopped-stage state (R3 §1b): the `STAGE`
//! waiting row and a frame whose machine's stage is down both say
//! `stage stopped · s start` where the width allows, `s` starts the
//! stage on the row's machine, Enter on the `Stage` row matches, and
//! an unreachable machine offers nothing. Neutral fixtures only
//! (`acme-shop`, `beta-tools`, `host-b`).

use super::super::studio_machines::two_machine_studio_app;
use super::*;
use crate::studio::StudioKey;

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

/// The two-machine fixture under an expanded Call Sheet — host-b's
/// remote document is part of it.
fn app_with_remote_sheet() -> App {
    let mut app = two_machine_studio_app();
    app.set_hollywood(Ok(Some(cfg(None))), std::path::Path::new("config.toml"));
    app.hollywood.mode = LayerMode::Expanded;
    app.area = (160, 24);
    app
}

#[tokio::test]
async fn the_sheet_says_stage_stopped_and_s_starts_it() {
    // host-b's stage terminal gone: the waiting row and the hint
    // both carry `stage stopped` / `s start`.
    let mut app = app_with_remote_sheet();
    let (_recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    drop_terminal(&mut app, "host-b/t9");
    let targets = app.hollywood_targets();
    let stage = Target::Stage(Some("host-b".into()), false);
    assert!(targets.contains(&stage), "{targets:?}");
    app.hollywood.selected = Some(stage);
    let text = screen_lines(&draw_to(&mut app, 160, 24)).join("\n");
    assert!(text.contains("STAGE STOPPED"), "{text}");
    key(&mut app, hc_vt::Key::Char('s'));
    assert_eq!(stage_start_pending(&app), Some(Some("host-b".into())));
}

#[tokio::test]
async fn s_on_a_studio_frame_starts_its_machines_stage() {
    // beta-tools' frame under a down host-b stage — the frame's `s`
    // routes `stage start` to host-b.
    let mut app = app_with_remote_sheet();
    let (_recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    drop_terminal(&mut app, "host-b/t9");
    app.hollywood.selected = Some(Target::Studio(StudioKey {
        machine: Some("host-b".into()),
        id: "beta-tools".into(),
    }));
    key(&mut app, hc_vt::Key::Char('s'));
    assert_eq!(stage_start_pending(&app), Some(Some("host-b".into())));
}

#[tokio::test]
async fn s_on_the_local_studio_frame_starts_the_local_stage() {
    // acme-shop's own stage down — the same `s` keeps `machine: None`.
    let mut app = app_with_remote_sheet();
    let (_recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    drop_terminal(&mut app, "t9");
    app.hollywood.selected = Some(Target::Studio(StudioKey {
        machine: None,
        id: "acme-shop".into(),
    }));
    key(&mut app, hc_vt::Key::Char('s'));
    assert_eq!(stage_start_pending(&app), Some(None));
}

#[tokio::test]
async fn enter_on_the_stage_row_starts_it() {
    let mut app = app_with_remote_sheet();
    let (_recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    drop_terminal(&mut app, "host-b/t9");
    app.hollywood.selected = Some(Target::Stage(Some("host-b".into()), false));
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(stage_start_pending(&app), Some(Some("host-b".into())));
}

#[tokio::test]
async fn an_unreachable_machine_offers_no_start() {
    // host-b gone: the `STAGE` row is a check (`offline: true`) and
    // `s` dispatches nothing.
    let mut app = app_with_remote_sheet();
    let (_recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    drop_terminal(&mut app, "host-b/t9");
    app.view.machines = Some(vec![crate::sidebar::fixture::machine(
        "host-b",
        "unreachable",
    )]);
    let stage = Target::Stage(Some("host-b".into()), true);
    assert!(app.hollywood_targets().contains(&stage));
    app.hollywood.selected = Some(stage);
    key(&mut app, hc_vt::Key::Char('s'));
    assert_eq!(stage_start_pending(&app), None);
    // The studio frame's `s` yields to the same check.
    app.hollywood.selected = Some(Target::Studio(StudioKey {
        machine: Some("host-b".into()),
        id: "beta-tools".into(),
    }));
    key(&mut app, hc_vt::Key::Char('s'));
    assert_eq!(stage_start_pending(&app), None);
}

#[tokio::test]
async fn s_on_a_frame_with_a_live_stage_stays_a_no_op() {
    let mut app = app_with_remote_sheet();
    let (_recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.hollywood.selected = Some(Target::Studio(StudioKey {
        machine: None,
        id: "acme-shop".into(),
    }));
    key(&mut app, hc_vt::Key::Char('s'));
    assert_eq!(stage_start_pending(&app), None);
}

//! W4b part 2 — the studio menu: `Free work areas (prune)…` with its
//! dry-run plan, and the empty-studio finish confirm that passes
//! `--force`.

use super::delete_studio::drain_dialog_fetch;
use super::lifecycle_fixtures::*;
use crate::studio_dialog::StudioDialog;
use crate::studio_menu::StudioAct;
use hc_vt::Key;

#[tokio::test]
async fn prune_shows_the_dry_run_then_runs_the_real_command() {
    let mut app = lifecycle_app();
    let recorded = scripted(&mut app);
    app.run_studio_act(&studio_menu(&app, "old-shop"), StudioAct::Prune);
    assert!(matches!(
        app.studio_dialog,
        Some(StudioDialog::Confirm(ref d)) if d.loading
    ));
    drain_dialog_fetch(&mut app).await;
    let Some(StudioDialog::Confirm(d)) = &app.studio_dialog else {
        panic!("the loaded confirm");
    };
    assert!(!d.loading);
    assert!(d.body.contains("work/t1: removed"), "{}", d.body);
    assert!(
        recorded
            .lock()
            .unwrap()
            .iter()
            .any(|a| a == &argv(&["studio", "prune", "old-shop", "--dry-run", "--as", "user"])),
        "the dry run ran"
    );
    key(&mut app, Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().last().unwrap(),
        &argv(&["studio", "prune", "old-shop", "--as", "user"])
    );
}

#[tokio::test]
async fn finishing_an_empty_studio_confirms_then_passes_force() {
    let mut app = lifecycle_app();
    let recorded = scripted(&mut app);
    app.run_studio_act(&studio_menu(&app, "fresh-one"), StudioAct::Finish);
    let Some(StudioDialog::Confirm(d)) = &app.studio_dialog else {
        panic!("the empty-studio confirm");
    };
    assert!(d.body.contains("no requirement, no tasks"), "{}", d.body);
    key(&mut app, Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().last().unwrap(),
        &argv(&[
            "studio",
            "finish",
            "--force",
            "--studio",
            "fresh-one",
            "--as",
            "user"
        ])
    );
}

#[tokio::test]
async fn finishing_a_worked_studio_keeps_the_reason_path_without_force() {
    let mut app = lifecycle_app();
    let recorded = scripted(&mut app);
    app.run_studio_act(&studio_menu(&app, "acme-shop"), StudioAct::Finish);
    assert!(matches!(app.studio_dialog, Some(StudioDialog::Reason(_))));
    key(&mut app, Key::Enter);
    let argv = recorded.lock().unwrap().last().unwrap().clone();
    assert!(!argv.iter().any(|a| a == "--force"), "{argv:?}");
}

//! W4b part 1 — the role menu: `Restart` and `Recast…` on the right
//! seats, no `Close` on a fixed one, the restart dialog's two argv
//! shapes, and the recast picker's effort rules.

use super::delete_studio::drain_dialog_fetch;
use super::lifecycle_fixtures::*;
use crate::studio_dialog::{RecastStep, StudioDialog};
use crate::studio_menu::StudioAct;
use hc_vt::Key;

#[tokio::test]
async fn a_fixed_seat_offers_restart_and_recast_but_no_close() {
    let app = lifecycle_app();
    let director = labels(&role_menu(&app, "director"));
    assert!(
        director.iter().any(|l| l == "Restart director"),
        "{director:?}"
    );
    assert!(director.iter().any(|l| l == "Recast…"), "{director:?}");
    assert!(
        !director.iter().any(|l| l.starts_with("Close")),
        "a fixed seat cannot close: {director:?}"
    );
    for role in ["producer", "writer"] {
        let rows = labels(&role_menu(&app, role));
        assert!(
            rows.iter().any(|l| l == &format!("Restart {role}")),
            "{rows:?}"
        );
        assert!(rows.iter().any(|l| l == "Recast…"), "{rows:?}");
        assert!(!rows.iter().any(|l| l.starts_with("Close")), "{rows:?}");
    }
    // An actor keeps its close and gains restart, not recast.
    let alice = labels(&role_menu(&app, "actor-alice"));
    assert!(alice.iter().any(|l| l == "Restart alice"), "{alice:?}");
    assert!(alice.iter().any(|l| l == "Close alice…"), "{alice:?}");
    assert!(!alice.iter().any(|l| l == "Recast…"), "{alice:?}");
}

#[tokio::test]
async fn the_restart_dialog_builds_both_argv_shapes() {
    let mut app = lifecycle_app();
    let recorded = scripted(&mut app);
    let menu = role_menu(&app, "director");

    // `claude` resumes: `Resume its session` leads, Enter runs it.
    app.run_studio_act(&menu, StudioAct::Restart);
    assert!(matches!(app.studio_dialog, Some(StudioDialog::Restart(_))));
    key(&mut app, Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().last().unwrap(),
        &argv(&[
            "restart",
            "--person",
            "director",
            "--studio",
            "acme-shop",
            "--as",
            "user"
        ])
    );

    // Down one choice and Enter: `Fresh start` adds `--fresh`.
    app.run_studio_act(&menu, StudioAct::Restart);
    key(&mut app, Key::ArrowDown);
    key(&mut app, Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().last().unwrap(),
        &argv(&[
            "restart",
            "--person",
            "director",
            "--fresh",
            "--studio",
            "acme-shop",
            "--as",
            "user"
        ])
    );
}

#[tokio::test]
async fn a_no_effort_runner_has_no_effort_row_in_the_picker() {
    // The caps decide the step list: cursor carries no effort flag.
    let mut app = lifecycle_app();
    scripted(&mut app);
    app.run_studio_act(&role_menu(&app, "director"), StudioAct::Recast);
    drain_dialog_fetch(&mut app).await;
    let Some(StudioDialog::Recast(d)) = app.studio_dialog.as_mut() else {
        panic!("the recast picker");
    };
    d.runner_sel = d
        .found()
        .iter()
        .position(|r| r.runner == "cursor")
        .expect("cursor in the fixture");
    d.reseat();
    d.step = RecastStep::Model;
    assert!(!d.has_effort(), "no effort row for a no-effort runner");
    assert_eq!(d.next_step(), RecastStep::Apply, "model skips to apply");
}

#[tokio::test]
async fn recast_a_required_effort_runner_lists_levels_and_sends_effort() {
    let mut app = lifecycle_app();
    let recorded = scripted(&mut app);
    app.run_studio_act(&role_menu(&app, "director"), StudioAct::Recast);
    drain_dialog_fetch(&mut app).await;
    let Some(StudioDialog::Recast(d)) = &app.studio_dialog else {
        panic!("the recast picker");
    };
    let levels: Vec<String> = d.efforts().into_iter().map(|e| e.label).collect();
    assert_eq!(levels, ["low", "medium", "high", "xhigh", "max"]);

    // Runner → model → effort → apply.
    for _ in 0..4 {
        key(&mut app, Key::Enter);
    }
    assert_eq!(
        recorded.lock().unwrap().last().unwrap(),
        &argv(&[
            "recast",
            "--person",
            "director",
            "--runner",
            "claude",
            "--model",
            "m1",
            "--effort",
            "high",
            "--studio",
            "acme-shop",
            "--as",
            "user"
        ])
    );
}

#[tokio::test]
async fn recast_a_no_effort_runner_skips_the_row_and_the_flag() {
    let mut app = lifecycle_app();
    let recorded = scripted(&mut app);
    app.run_studio_act(&role_menu(&app, "director"), StudioAct::Recast);
    drain_dialog_fetch(&mut app).await;
    // Second row: cursor (no effort).
    key(&mut app, Key::ArrowDown);
    key(&mut app, Key::Enter); // model
    key(&mut app, Key::Enter); // apply — no effort step
    key(&mut app, Key::Enter); // run
    let argv = recorded.lock().unwrap().last().unwrap().clone();
    assert!(!argv.iter().any(|a| a == "--effort"), "{argv:?}");
    assert!(
        argv.windows(2).any(|w| w == ["--runner", "cursor"]),
        "{argv:?}"
    );
}

#[tokio::test]
async fn recast_an_optional_effort_runner_allows_the_empty_choice() {
    let mut app = lifecycle_app();
    let recorded = scripted(&mut app);
    app.run_studio_act(&role_menu(&app, "director"), StudioAct::Recast);
    drain_dialog_fetch(&mut app).await;
    // Third row: devin (optional effort, no levels).
    key(&mut app, Key::ArrowDown);
    key(&mut app, Key::ArrowDown);
    key(&mut app, Key::Enter); // model
    key(&mut app, Key::Enter); // effort
    let Some(StudioDialog::Recast(d)) = &app.studio_dialog else {
        panic!("the effort step");
    };
    assert_eq!(d.efforts().len(), 1, "the empty choice alone");
    assert_eq!(d.efforts()[0].value, None);
    key(&mut app, Key::Enter); // apply
    key(&mut app, Key::Enter); // run
    let argv = recorded.lock().unwrap().last().unwrap().clone();
    assert!(!argv.iter().any(|a| a == "--effort"), "{argv:?}");
}

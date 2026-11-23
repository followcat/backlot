//! Pre-confirm tests for `Delete studio…` when `--dry-run` already lists `kept` items.

use super::delete_studio::{drain_dialog_fetch, press, report_json};
use super::studio::two_studio_app;
use super::*;
use crate::studio::StudioKey;
use crate::studio_dialog::StudioDialog;

const KEPT_ITEMS: &[&str] = &[
    "kept worktree work/t1 — uncommitted changes",
    "kept branch backlot/acme-shop/t1 — worktree kept",
];

#[tokio::test]
async fn dry_run_kept_opens_force_confirm_before_mutating_and_esc_cancels_cleanly() {
    let mut app = two_studio_app();
    app.area = (120, 24);
    let recorded = Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let rec_clone = Arc::clone(&recorded);
    app.set_ns_run(Arc::new(move |_, argv, _| {
        rec_clone.lock().unwrap().push(argv.clone());
        let dry = argv.iter().any(|a| a == "--dry-run");
        Box::pin(async move {
            assert!(dry, "unexpected mutating command: {argv:?}");
            Ok(report_json("acme-shop", true, false, false, KEPT_ITEMS))
        })
    }));

    app.delete_studio_open(&StudioKey::local("acme-shop"));
    drain_dialog_fetch(&mut app).await;
    assert_eq!(recorded.lock().unwrap().len(), 1);

    // Accepting the first confirmation opens the second `--force` confirmation
    // without running any mutating command.
    press(&mut app, hc_vt::Key::Enter, hc_vt::Mods::NONE);
    assert_eq!(recorded.lock().unwrap().len(), 1);
    let Some(StudioDialog::Confirm(force_dlg)) = &app.studio_dialog else {
        panic!("expected second --force confirmation dialog");
    };
    assert!(
        force_dlg.title.starts_with("Force delete studio?"),
        "{}",
        force_dlg.title
    );
    assert!(
        force_dlg.body.contains("kept worktree work/t1"),
        "{}",
        force_dlg.body
    );

    // Cancelling at the second step runs no additional command and keeps the studio.
    press(&mut app, hc_vt::Key::Escape, hc_vt::Mods::NONE);
    assert!(app.studio_dialog.is_none());
    assert_eq!(recorded.lock().unwrap().len(), 1);
    assert!(
        crate::panel::data::studio(&app.panel_summaries(), &StudioKey::local("acme-shop"))
            .is_some()
    );
}

#[tokio::test]
async fn dry_run_kept_double_confirm_runs_only_one_forced_delete() {
    let mut app = two_studio_app();
    app.area = (120, 24);
    let recorded = Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let rec_clone = Arc::clone(&recorded);
    app.set_ns_run(Arc::new(move |_, argv, _| {
        rec_clone.lock().unwrap().push(argv.clone());
        let dry = argv.iter().any(|a| a == "--dry-run");
        let force = argv.iter().any(|a| a == "--force");
        Box::pin(async move {
            if dry {
                Ok(report_json("acme-shop", true, false, false, KEPT_ITEMS))
            } else {
                Ok(report_json("acme-shop", false, force, force, &[]))
            }
        })
    }));

    app.delete_studio_open(&StudioKey::local("acme-shop"));
    drain_dialog_fetch(&mut app).await;

    // First confirm transitions straight to Force delete studio?.
    press(&mut app, hc_vt::Key::Enter, hc_vt::Mods::NONE);
    assert_eq!(recorded.lock().unwrap().len(), 1);
    assert!(matches!(
        app.studio_dialog,
        Some(StudioDialog::Confirm(ref d)) if d.title.starts_with("Force delete studio?")
    ));

    // Second confirm runs exactly one mutating command with `--force`.
    press(&mut app, hc_vt::Key::Enter, hc_vt::Mods::NONE);
    assert!(app.studio_dialog.is_none());
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);

    let calls = recorded.lock().unwrap();
    assert_eq!(
        calls.len(),
        2,
        "expected dry-run + forced delete: {calls:?}"
    );
    assert_eq!(
        calls[0],
        [
            "studio",
            "delete",
            "acme-shop",
            "--dry-run",
            "--json",
            "--as",
            "user"
        ]
    );
    assert_eq!(
        calls[1],
        [
            "studio",
            "delete",
            "acme-shop",
            "--json",
            "--as",
            "user",
            "--force"
        ]
    );
    assert_eq!(
        app.action_toast.as_ref().map(|t| t.title.as_str()),
        Some("deleted studio acme-shop")
    );
    assert!(
        crate::panel::data::studio(&app.panel_summaries(), &StudioKey::local("acme-shop"))
            .is_none()
    );
}

#[tokio::test]
async fn dry_run_empty_kept_still_runs_non_force_and_falls_back_on_raced_kept() {
    let mut app = two_studio_app();
    app.area = (120, 24);
    let recorded = Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let rec_clone = Arc::clone(&recorded);
    app.set_ns_run(Arc::new(move |_, argv, _| {
        rec_clone.lock().unwrap().push(argv.clone());
        let dry = argv.iter().any(|a| a == "--dry-run");
        let force = argv.iter().any(|a| a == "--force");
        Box::pin(async move {
            if dry {
                Ok(report_json("acme-shop", true, false, false, &[]))
            } else if force {
                Ok(report_json("acme-shop", false, true, true, &[]))
            } else {
                Err(report_json("acme-shop", false, false, false, KEPT_ITEMS))
            }
        })
    }));

    app.delete_studio_open(&StudioKey::local("acme-shop"));
    drain_dialog_fetch(&mut app).await;

    // First confirm runs non-`--force` `studio delete`.
    press(&mut app, hc_vt::Key::Enter, hc_vt::Mods::NONE);
    assert_eq!(
        recorded.lock().unwrap()[1],
        ["studio", "delete", "acme-shop", "--json", "--as", "user"]
    );

    // If state changed meanwhile and `studio delete` fails with kept items,
    // the fallback `--force` dialog still opens and submits `--force`.
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    assert!(matches!(
        app.studio_dialog,
        Some(StudioDialog::Confirm(ref d)) if d.title.starts_with("Force delete studio?")
    ));

    press(&mut app, hc_vt::Key::Enter, hc_vt::Mods::NONE);
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);

    let calls = recorded.lock().unwrap();
    assert_eq!(calls.len(), 3, "calls: {calls:?}");
    assert_eq!(
        calls[2],
        [
            "studio",
            "delete",
            "acme-shop",
            "--json",
            "--as",
            "user",
            "--force"
        ]
    );
    assert_eq!(
        app.action_toast.as_ref().map(|t| t.title.as_str()),
        Some("deleted studio acme-shop")
    );
}

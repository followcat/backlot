//! App tests for real `Delete studio…` dialog, toasts, and immediate eviction.

use super::studio::two_studio_app;
use super::*;
use crate::panel::PanelMode;
use crate::studio::{StudioFocus, StudioKey};
use crate::studio_dialog::StudioDialog;

pub(super) fn report_json(
    studio: &str,
    dry_run: bool,
    force: bool,
    archived: bool,
    kept: &[&str],
) -> String {
    serde_json::json!({
        "studio": studio,
        "dry_run": dry_run,
        "force": force,
        "finished": !dry_run,
        "closed_terminals": if archived || dry_run { vec![format!("bl-{studio}-producer")] } else { vec![] },
        "removed_worktrees": [],
        "pruned_worktrees": [],
        "deleted_branches": [],
        "kept": kept,
        "archive": format!("archive/{studio}-20261123T060000Z"),
        "archived": archived,
    })
    .to_string()
}

pub(super) async fn drain_dialog_fetch(app: &mut App) {
    for _ in 0..20 {
        tokio::task::yield_now().await;
        while let Ok(reply) = app.dialog_fetch_rx.try_recv() {
            app.on_dialog_fetch(reply);
        }
    }
}

#[tokio::test]
async fn dry_run_failure_closes_dialog_and_shows_error_toast() {
    let mut app = two_studio_app();
    app.set_ns_run(Arc::new(|_, _, _| {
        Box::pin(async { Err("cannot finish; open tasks: t1".into()) })
    }));
    app.delete_studio_open(&StudioKey::local("acme-shop"));
    assert!(matches!(
        app.studio_dialog,
        Some(StudioDialog::Confirm(ref d)) if d.loading
    ));
    drain_dialog_fetch(&mut app).await;
    assert!(app.studio_dialog.is_none());
    let toast = app.action_toast.as_ref().expect("error toast");
    assert!(toast.fail);
    assert!(toast.body.contains("open tasks: t1"), "{}", toast.body);
}

#[tokio::test]
async fn delete_failure_never_shows_deleted_toast_and_keeps_studio() {
    let mut app = two_studio_app();
    app.set_ns_run(Arc::new(|_, _, _| {
        Box::pin(async { Err("terminal bl-acme-shop-producer did not exit".into()) })
    }));
    app.run_action(crate::studio_cmds::delete_studio(
        &StudioKey::local("acme-shop"),
        false,
    ));
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    let toast = app.action_toast.as_ref().expect("failure toast");
    assert!(toast.fail);
    assert!(!toast.title.contains("deleted studio"), "{}", toast.title);
    assert!(toast.body.contains("did not exit"), "{}", toast.body);
    assert!(
        crate::panel::data::studio(&app.panel_summaries(), &StudioKey::local("acme-shop"))
            .is_some()
    );
}

#[tokio::test]
async fn delete_unarchived_ok_never_claims_deleted() {
    let mut app = two_studio_app();
    app.set_ns_run(Arc::new(|_, _, _| {
        Box::pin(async {
            Ok(serde_json::json!({
                "studio": "acme-shop",
                "dry_run": false,
                "force": false,
                "finished": true,
                "closed_terminals": [],
                "removed_worktrees": [],
                "pruned_worktrees": [],
                "deleted_branches": [],
                "kept": ["kept worktree work/t1 — uncommitted changes"],
                "archive": "archive/acme-shop-20261123T060000Z",
                "archived": false,
            })
            .to_string())
        })
    }));
    app.run_action(crate::studio_cmds::delete_studio(
        &StudioKey::local("acme-shop"),
        false,
    ));
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    let toast = app.action_toast.as_ref().expect("failure toast");
    assert!(toast.fail);
    assert!(!toast.title.contains("deleted studio"), "{}", toast.title);
    assert!(toast.body.contains("kept worktree"), "{}", toast.body);
}

#[tokio::test]
async fn delete_success_evicts_studio_from_sidebar_dock_call_sheet_and_advances_focus() {
    let mut app = two_studio_app();
    super::endpoint_nav::pending_attach(&mut app);
    app.area = (120, 24);
    app.panel.mode = PanelMode::Expanded;
    app.hollywood.mode = crate::hollywood::LayerMode::Expanded;
    app.studio
        .remembered
        .insert(StudioKey::local("acme-shop"), "actor-alice".into());
    app.panel
        .draft_mut(&StudioKey::local("acme-shop"))
        .insert("draft text");

    app.set_ns_run(Arc::new(|_, argv, _| {
        let id = argv[2].clone();
        Box::pin(async move { Ok(report_json(&id, false, false, true, &[])) })
    }));

    // Delete `acme-shop`: immediately evicted and focus advances to `night-owl`.
    app.run_action(crate::studio_cmds::delete_studio(
        &StudioKey::local("acme-shop"),
        false,
    ));
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);

    assert_eq!(
        app.action_toast.as_ref().map(|t| t.title.as_str()),
        Some("deleted studio acme-shop")
    );
    assert!(
        crate::panel::data::studio(&app.panel_summaries(), &StudioKey::local("acme-shop"))
            .is_none()
    );
    assert!(
        !app.studio
            .remembered
            .contains_key(&StudioKey::local("acme-shop"))
    );
    assert!(app.panel.draft(&StudioKey::local("acme-shop")).is_none());
    assert_eq!(app.studio.current, Some(StudioKey::local("night-owl")));
    assert_eq!(
        app.studio.focus,
        Some(StudioFocus::Studio(StudioKey::local("night-owl")))
    );

    // Dismiss the toast so we verify acme-shop is gone from sidebar, dock, and Call Sheet.
    app.action_toast = None;
    let backend = draw_to(&mut app, 120, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(
        !text.contains("acme-shop"),
        "evicted from all surfaces:\n{text}"
    );
    assert!(text.contains("night-owl"), "{text}");

    // Delete remaining studios -> focus becomes None.
    for id in ["night-owl", "ghost"] {
        app.run_action(crate::studio_cmds::delete_studio(
            &StudioKey::local(id),
            false,
        ));
        let reply = app.action_rx.recv().await.unwrap();
        app.on_action_reply(reply);
    }
    assert!(app.studio.current.is_none());
    assert!(app.studio.focus.is_none());
    assert!(crate::panel::data::studios(&app.panel_summaries()).is_empty());
}

pub(super) fn press(app: &mut App, k: hc_vt::Key, mods: hc_vt::Mods) {
    app.on_input(
        crate::app::input::UiEvent::Key(term_key(k, mods, hc_vt::Action::Press)),
        app.area.0,
    );
}

#[tokio::test]
async fn delete_studio_keyboard_flow_and_force_second_confirm() {
    use super::studio::alt;
    let mut app = two_studio_app();
    super::endpoint_nav::pending_attach(&mut app);
    app.area = (120, 24);
    let recorded = Arc::new(std::sync::Mutex::new(
        Vec::<(Option<String>, Vec<String>)>::new(),
    ));
    let rec_clone = Arc::clone(&recorded);
    app.set_ns_run(Arc::new(move |machine, argv, _| {
        rec_clone.lock().unwrap().push((machine, argv.clone()));
        let dry = argv.iter().any(|a| a == "--dry-run");
        let studio = argv.get(2).cloned().unwrap_or_else(|| "night-owl".into());
        Box::pin(async move { Ok(report_json(&studio, dry, false, !dry, &[])) })
    }));

    // Keyboard-only: Alt+Right -> Ctrl+B m -> Delete studio… -> Enter.
    alt(&mut app, hc_vt::Key::ArrowRight);
    press(&mut app, hc_vt::Key::Char('b'), hc_vt::Mods::CTRL);
    press(&mut app, hc_vt::Key::Char('m'), hc_vt::Mods::NONE);
    let idx = app
        .menu
        .as_ref()
        .unwrap()
        .items()
        .iter()
        .position(|i| i.label == "Delete studio…")
        .expect("Delete studio… in studio menu");
    for _ in 0..idx {
        press(&mut app, hc_vt::Key::ArrowDown, hc_vt::Mods::NONE);
    }
    press(&mut app, hc_vt::Key::Enter, hc_vt::Mods::NONE);

    let Some(StudioDialog::Confirm(loading_dlg)) = &app.studio_dialog else {
        panic!("expected loading confirm dialog");
    };
    assert!(loading_dlg.loading);
    press(&mut app, hc_vt::Key::Enter, hc_vt::Mods::NONE);
    assert!(
        app.studio_dialog.is_some(),
        "loading blocks premature confirm"
    );

    drain_dialog_fetch(&mut app).await;
    let Some(StudioDialog::Confirm(confirm)) = &app.studio_dialog else {
        panic!("expected loaded confirm dialog");
    };
    assert!(!confirm.loading);
    assert!(
        confirm.body.contains("Closes 1 terminal(s)"),
        "{}",
        confirm.body
    );
    assert!(
        confirm.body.contains("archive/night-owl-20261123T060000Z"),
        "{}",
        confirm.body
    );
    assert!(
        confirm.body.contains("Your repository is not touched."),
        "{}",
        confirm.body
    );

    // Cancelling runs no delete mutation.
    press(&mut app, hc_vt::Key::Escape, hc_vt::Mods::NONE);
    assert!(app.studio_dialog.is_none());
    assert_eq!(recorded.lock().unwrap().len(), 1);

    // Confirm runs `studio delete night-owl --json --as user`.
    app.delete_studio_open(&StudioKey::local("night-owl"));
    drain_dialog_fetch(&mut app).await;
    press(&mut app, hc_vt::Key::Enter, hc_vt::Mods::NONE);
    assert_eq!(
        recorded.lock().unwrap().last().unwrap().1,
        ["studio", "delete", "night-owl", "--json", "--as", "user"]
    );
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    assert_eq!(
        app.action_toast.as_ref().map(|t| t.title.as_str()),
        Some("deleted studio night-owl")
    );

    // Kept dirty worktrees on exit 1 -> second `--force` confirm.
    let rec2 = Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let rec2_clone = Arc::clone(&rec2);
    app.set_ns_run(Arc::new(move |_, argv, _| {
        rec2_clone.lock().unwrap().push(argv.clone());
        let force = argv.iter().any(|a| a == "--force");
        Box::pin(async move {
            if force {
                Ok(report_json("acme-shop", false, true, true, &[]))
            } else {
                Err(report_json(
                    "acme-shop",
                    false,
                    false,
                    false,
                    &[
                        "kept worktree work/t1 — uncommitted changes",
                        "kept branch backlot/acme-shop/t1 — worktree kept",
                    ],
                ))
            }
        })
    }));
    app.run_action(crate::studio_cmds::delete_studio(
        &StudioKey::local("acme-shop"),
        false,
    ));
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    let Some(StudioDialog::Confirm(force_dlg)) = &app.studio_dialog else {
        panic!("expected second --force confirmation dialog");
    };
    assert!(force_dlg.body.contains("kept worktree work/t1"));
    assert!(app.action_toast.as_ref().is_some_and(|t| t.fail));
    press(&mut app, hc_vt::Key::Enter, hc_vt::Mods::NONE);
    assert_eq!(
        rec2.lock().unwrap()[1],
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
}

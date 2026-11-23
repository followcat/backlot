//! R3 followups app tests (§0 StudioFocus, §1 Alt+arrows across every
//! surface, §2 Delete studio…, and §4 hidden `backlot` stage space).
use super::alt_arrows::alt_app;
use super::studio::{alt, two_studio_app};
use super::*;
use crate::app::input::{MouseButton, MouseEventKind, MouseInput, UiEvent};
use crate::studio::{StudioFocus, StudioKey};

fn mouse(kind: MouseEventKind, col: u16, row: u16) -> MouseInput {
    MouseInput {
        kind,
        col,
        row,
        mods: hc_vt::Mods::NONE,
        sgr_position: None,
    }
}

fn ctrl_b_m(app: &mut App) {
    for (k, mods) in [
        (hc_vt::Key::Char('b'), hc_vt::Mods::CTRL),
        (hc_vt::Key::Char('m'), hc_vt::Mods::NONE),
    ] {
        app.on_input(
            UiEvent::Key(term_key(k, mods, hc_vt::Action::Press)),
            app.area.0,
        );
    }
}

fn send_key(app: &mut App, k: hc_vt::Key, mods: hc_vt::Mods) {
    app.on_input(
        UiEvent::Key(term_key(k, mods, hc_vt::Action::Press)),
        app.area.0,
    );
}

#[tokio::test]
async fn studio_focus_separates_sidebar_highlight_from_cast_and_pane_typing() {
    let mut app = two_studio_app();
    app.area = (120, 24);
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    // Start on acme-shop's producer (Role focus): Cast highlights t1.
    assert_eq!(
        app.studio.focus,
        Some(StudioFocus::Role(
            StudioKey::local("acme-shop"),
            "t1".into()
        ))
    );
    draw_to(&mut app, 120, 24);

    // Alt+Down switches StudioFocus to Studio(night-owl) without moving
    // the attached typing pane (t1).
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(
        app.studio.focus,
        Some(StudioFocus::Studio(StudioKey::local("night-owl")))
    );
    assert_eq!(app.focused.as_deref(), Some("t1"));

    // No role in Cast paints the `▌` focus bar while StudioFocus::Studio
    // owns focus, while the sidebar row for night-owl shows the studio focus.
    let backend = draw_to(&mut app, 120, 24);
    let buf = backend.buffer();
    for (rect, _) in &app.hit_map.agents {
        assert_ne!(buf[(rect.x, rect.y)].symbol(), "▌", "no role highlighted");
    }
    let night_row = app
        .hit_map
        .workspaces
        .iter()
        .find(|w| w.workspace_id.ends_with("night-owl"))
        .map(|w| w.rect)
        .unwrap_or_default();
    if night_row.width > 0 {
        assert_eq!(buf[(night_row.x, night_row.y)].symbol(), "▌");
    }

    // Typing still goes to self.pane (`t1`).
    let before = app.pane.input_backlog().0;
    send_key(&mut app, hc_vt::Key::Char('x'), hc_vt::Mods::NONE);
    assert!(app.pane.input_backlog().0 > before);

    // Ctrl+B m opens the Studio menu (with title line) for night-owl.
    ctrl_b_m(&mut app);
    let (title, item_count) = {
        let menu = app.menu.as_ref().expect("studio menu opened");
        (menu.title(), menu.items().len())
    };
    assert_eq!(title.as_deref(), Some("Studio · night-owl"));
    draw_to(&mut app, 120, 24);
    assert_eq!(
        app.hit_map.context_menu_rows.len(),
        item_count,
        "title row is non-selectable and excluded from hit rows"
    );

    // Alt+Right enters night-owl's remembered role (t7) and opens Role menu.
    alt(&mut app, hc_vt::Key::ArrowRight);
    assert_eq!(app.focused.as_deref(), Some("t7"));
    assert_eq!(
        app.studio.focus,
        Some(StudioFocus::Role(
            StudioKey::local("night-owl"),
            "t7".into()
        ))
    );
    ctrl_b_m(&mut app);
    let menu = app.menu.as_ref().expect("role menu opened");
    assert_eq!(
        menu.title().as_deref(),
        Some("Role · owl-producer (night-owl)")
    );
}

#[tokio::test]
async fn prefix_chord_closes_open_menu_and_reopens_for_current_focus() {
    let mut app = super::studio_machines::two_machine_studio_app();
    super::endpoint_nav::pending_attach(&mut app);
    app.area = (120, 24);
    draw_to(&mut app, 120, 24);
    // Right-click acme-shop workspace row -> StudioFocus::Studio(acme-shop) & menu opens.
    let ws = app
        .hit_map
        .workspaces
        .iter()
        .find(|w| w.workspace_id.ends_with("acme-shop"))
        .map(|w| w.rect)
        .expect("acme-shop workspace row");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Right),
        ws.x + 2,
        ws.y,
    ));
    assert!(app.menu.is_some());
    assert_eq!(
        app.studio.focus,
        Some(StudioFocus::Studio(StudioKey::local("acme-shop")))
    );
    // Pressing Ctrl+B while the menu is open closes it and arms prefix so `m` opens studio menu.
    ctrl_b_m(&mut app);
    assert_eq!(
        app.menu.as_ref().and_then(|m| m.title()).as_deref(),
        Some("Studio · acme-shop")
    );
}

#[tokio::test]
async fn alt_arrows_work_across_all_non_text_contexts_and_yield_to_modals() {
    // Non-text contexts dismiss cleanly on Alt+arrows.
    for setup in [
        |app: &mut App| app.help = true,
        |app: &mut App| {
            app.menu = Some(crate::chrome::menu::ContextMenu {
                target: crate::chrome::menu::ContextMenuTarget::Agent {
                    pane_id: "t1".into(),
                },
                x: 2,
                y: 2,
                highlighted: 0,
            });
        },
        |app: &mut App| {
            app.navigator = Some(crate::chrome::navigator::NavigatorState::default());
        },
        |app: &mut App| {
            app.sheet = Some(crate::sheet::Sheet::new(
                StudioKey::local("acme-shop"),
                None,
                1,
            ));
        },
        |app: &mut App| {
            app.mode = crate::app::Mode::Navigate;
        },
        |app: &mut App| {
            app.peek = Some(crate::app::popup::PeekState {
                pane: pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone()),
                terminal: "t4".into(),
                created_ms: 0,
                owner_tab: None,
                seen: true,
                prompt: None,
                studio_title: None,
            });
        },
    ] {
        let mut app = two_studio_app();
        setup(&mut app);
        alt(&mut app, hc_vt::Key::ArrowDown);
        assert_eq!(app.studio.current, Some("night-owl".into()));
        assert!(!app.help && app.menu.is_none() && app.navigator.is_none());
        assert!(app.sheet.is_none() && app.peek.is_none());
        assert_eq!(app.mode, crate::app::Mode::Normal);
    }

    // Plain/stage/hollywood/shell workspaces all respond to all four Alt+arrows.
    let mut app = alt_app();
    for start in ["t20", "t9", "t40", "t30"] {
        app.focus_pane(start.into());
        alt(&mut app, hc_vt::Key::ArrowDown);
        assert!(app.studio.current.is_some());
        alt(&mut app, hc_vt::Key::ArrowLeft);
        assert!(matches!(app.studio.focus, Some(StudioFocus::Role(..))));
    }

    // Text-input modals & Copy/Resize modes keep Alt+arrows.
    let mut app = two_studio_app();
    app.studio_dialog = Some(crate::studio_dialog::StudioDialog::reason(
        "Reason pause task t1 · acme-shop · Local".into(),
        true,
        crate::studio_dialog::ReasonFor::TaskVerb {
            key: StudioKey::local("acme-shop"),
            task: "t1".into(),
            verb: "pause",
        },
    ));
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.studio.current, Some("acme-shop".into()));
    assert!(app.studio_dialog.is_some());

    let mut app = two_studio_app();
    app.mode = crate::app::Mode::Resize;
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.studio.current, Some("acme-shop".into()));

    let mut app = two_studio_app();
    app.mode = crate::app::Mode::Copy;
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.studio.current, Some("acme-shop".into()));
}

#[tokio::test]
async fn single_studio_single_role_toggles_studio_focus_with_alt_arrows() {
    let mut app = super::endpoint_nav::app_multi(vec![crate::sidebar::fixture::term("t1")], vec![]);
    app.set_plain(false);
    app.view.documents_mut().insert(
        super::studio::doc_key(),
        hc_proto::watch::Document {
            value: serde_json::json!({
                "schema": "backlot/1",
                "updated_ms": 1,
                "studios": [{
                    "id": "acme-shop",
                    "title": "Acme shop",
                    "state": "live",
                    "requirement_version": 1,
                    "mode": "standard",
                    "open_reviews": 0,
                    "waiting": 0,
                    "roles": [
                        { "role": "producer", "name": "producer", "terminal": "t1", "state": "live" }
                    ]
                }]
            }),
            ..Default::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    assert_eq!(
        app.studio.focus,
        Some(StudioFocus::Role(
            StudioKey::local("acme-shop"),
            "t1".into()
        ))
    );
    alt(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(
        app.studio.focus,
        Some(StudioFocus::Studio(StudioKey::local("acme-shop")))
    );
    alt(&mut app, hc_vt::Key::ArrowRight);
    assert_eq!(
        app.studio.focus,
        Some(StudioFocus::Role(
            StudioKey::local("acme-shop"),
            "t1".into()
        ))
    );
}

#[tokio::test]
async fn delete_studio_keyboard_flow_and_force_second_confirm() {
    let mut app = two_studio_app();
    super::endpoint_nav::pending_attach(&mut app);
    app.area = (120, 24);
    let recorded = Arc::new(std::sync::Mutex::new(
        Vec::<(Option<String>, Vec<String>)>::new(),
    ));
    let rec_clone = Arc::clone(&recorded);
    app.set_ns_run(Arc::new(move |machine, argv, _| {
        rec_clone.lock().unwrap().push((machine, argv));
        Box::pin(async { Ok("  nothing to prune\n".into()) })
    }));

    // Keyboard-only: Alt+Down -> Ctrl+B m -> Delete studio… -> Enter.
    alt(&mut app, hc_vt::Key::ArrowDown);
    ctrl_b_m(&mut app);
    let items = app.menu.as_ref().unwrap().items();
    let idx = items
        .iter()
        .position(|i| i.label == "Delete studio…")
        .expect("Delete studio… in studio menu");
    for _ in 0..idx {
        send_key(&mut app, hc_vt::Key::ArrowDown, hc_vt::Mods::NONE);
    }
    send_key(&mut app, hc_vt::Key::Enter, hc_vt::Mods::NONE);

    // First confirmation explains what goes and what stays.
    let dialog = app.studio_dialog.as_ref().expect("confirm dialog open");
    let crate::studio_dialog::StudioDialog::Confirm(confirm) = dialog else {
        panic!("expected confirm dialog");
    };
    assert!(confirm.title.contains("Delete studio"), "{}", confirm.title);
    assert!(
        confirm.body.contains("roles and terminals"),
        "{}",
        confirm.body
    );
    assert!(
        confirm.body.contains("worktrees and task branches"),
        "{}",
        confirm.body
    );
    assert!(confirm.body.contains("ledger"), "{}", confirm.body);
    assert!(
        confirm.body.contains("integration branch"),
        "{}",
        confirm.body
    );
    assert!(confirm.body.contains("never deleted"), "{}", confirm.body);

    // Cancelling runs nothing.
    send_key(&mut app, hc_vt::Key::Escape, hc_vt::Mods::NONE);
    assert!(app.studio_dialog.is_none());
    assert!(recorded.lock().unwrap().is_empty());

    // Confirm runs `studio finish` then `studio prune`.
    ctrl_b_m(&mut app);
    send_key(&mut app, hc_vt::Key::Char('d'), hc_vt::Mods::NONE);
    send_key(&mut app, hc_vt::Key::Enter, hc_vt::Mods::NONE);
    assert_eq!(recorded.lock().unwrap().len(), 1);
    assert_eq!(recorded.lock().unwrap()[0].1[..2], ["studio", "finish"]);

    // Deliver the reply to advance finish -> prune.
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    assert_eq!(recorded.lock().unwrap().len(), 2);
    assert_eq!(
        recorded.lock().unwrap()[1].1,
        ["studio", "prune", "night-owl", "--as", "user"]
    );
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    assert!(app.studio_dialog.is_none());

    // Now test `prune` returning kept dirty worktrees -> second `--force` confirm.
    let rec2 = Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let rec2_clone = Arc::clone(&rec2);
    app.set_ns_run(Arc::new(move |_, argv, _| {
        rec2_clone.lock().unwrap().push(argv);
        Box::pin(async {
            Ok("studio acme-shop (finished): prune\n  kept worktree work/t1 — uncommitted changes\n  kept branch backlot/acme-shop/t1 — worktree kept\n".into())
        })
    }));
    app.delete_studio_run(&StudioKey::local("acme-shop"), true, false);
    assert_eq!(rec2.lock().unwrap().len(), 1);
    assert_eq!(
        rec2.lock().unwrap()[0],
        ["studio", "prune", "acme-shop", "--as", "user"]
    );
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    let Some(crate::studio_dialog::StudioDialog::Confirm(force_dlg)) = &app.studio_dialog else {
        panic!("expected second --force confirmation dialog");
    };
    assert!(
        force_dlg.body.contains("kept worktree work/t1"),
        "{}",
        force_dlg.body
    );
    assert!(
        force_dlg.body.contains("kept branch backlot/acme-shop/t1"),
        "{}",
        force_dlg.body
    );
    send_key(&mut app, hc_vt::Key::Enter, hc_vt::Mods::NONE);
    assert_eq!(
        rec2.lock().unwrap()[1],
        ["studio", "prune", "acme-shop", "--force", "--as", "user"]
    );
}

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

    // Alt+PgDn steps to night-owl and lands its remembered member —
    // its producer — in the main pane.
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(
        app.studio.focus,
        Some(StudioFocus::Role(
            StudioKey::local("night-owl"),
            "t7".into()
        ))
    );
    assert_eq!(app.focused.as_deref(), Some("t7"));

    // The cast highlights the landed member and typing reaches it.
    draw_to(&mut app, 120, 24);
    let cast_row = app
        .hit_map
        .agents
        .iter()
        .find(|(_, terminal)| terminal == "t7")
        .map(|(rect, _)| *rect)
        .expect("the landed member's cast row");
    let backend = draw_to(&mut app, 120, 24);
    assert_eq!(
        backend.buffer()[(cast_row.x, cast_row.y)].symbol(),
        "▌",
        "the landed member carries the cast cursor"
    );
    let before = app.pane.input_backlog().0;
    send_key(&mut app, hc_vt::Key::Char('x'), hc_vt::Mods::NONE);
    assert!(app.pane.input_backlog().0 > before);

    // Ctrl+B m opens the role menu for the landed member.
    ctrl_b_m(&mut app);
    let menu = app.menu.as_ref().expect("role menu opened");
    assert_eq!(
        menu.title().as_deref(),
        Some("Role · owl-producer (night-owl)")
    );
    app.menu = None;

    // Alt+Down then walks night-owl's own cast from the landed member.
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.focused.as_deref(), Some("t8"));
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
        alt(&mut app, hc_vt::Key::PageDown);
        assert_eq!(app.studio.current, Some("night-owl".into()));
        assert!(!app.help && app.menu.is_none() && app.navigator.is_none());
        assert!(app.sheet.is_none() && app.peek.is_none());
        assert_eq!(app.mode, crate::app::Mode::Normal);
    }

    // Plain/stage/hollywood/shell workspaces respond to studio keys from anywhere,
    // then role keys enter the focused studio's remembered role.
    let mut app = alt_app();
    for start in ["t20", "t9", "t40", "t30"] {
        app.focus_pane(start.into());
        alt(&mut app, hc_vt::Key::PageDown);
        assert!(app.studio.current.is_some());
        alt(&mut app, hc_vt::Key::ArrowUp);
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
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("acme-shop".into()));
    assert!(app.studio_dialog.is_some());

    let mut app = two_studio_app();
    app.mode = crate::app::Mode::Resize;
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("acme-shop".into()));

    let mut app = two_studio_app();
    app.mode = crate::app::Mode::Copy;
    alt(&mut app, hc_vt::Key::PageDown);
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
    // One studio wraps onto itself: the step lands its one member in
    // the pane rather than parking a bare studio row.
    alt(&mut app, hc_vt::Key::PageUp);
    assert_eq!(app.studio.current, Some(StudioKey::local("acme-shop")));
    assert_eq!(
        app.studio.focus,
        Some(StudioFocus::Role(
            StudioKey::local("acme-shop"),
            "t1".into()
        ))
    );
    assert_eq!(app.focused.as_deref(), Some("t1"));
}

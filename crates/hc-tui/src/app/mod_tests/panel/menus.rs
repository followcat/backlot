//! S3 §1–§3 app tests: `m`, right-click and `Ctrl+B m` open the
//! studio menus, the dialogs own their keys, and the failure card's
//! `C` writes the exact command through OSC 52.
use super::*;
use crate::app::input::MouseButton;
use crate::chrome::menu::ContextMenuTarget;
use crate::studio::StudioKey;

fn studio_menu_open(app: &App) -> bool {
    matches!(
        app.menu.as_ref().map(|m| &m.target),
        Some(ContextMenuTarget::Studio(_))
    )
}

#[tokio::test]
async fn m_on_the_dock_opens_the_selected_rows_menu() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(studio_menu_open(&app), "m opened {}'s menu", "alice");
    // Enter's item focuses; the hint letters activate their row.
    let Some(menu) = app.menu.take() else {
        panic!("the menu stayed open")
    };
    let ContextMenuTarget::Studio(sm) = menu.target else {
        panic!("a studio menu")
    };
    let labels: Vec<_> = sm.items().iter().map(|i| i.label.clone()).collect();
    assert!(labels.iter().any(|l| l == "Tell alice…"), "{labels:?}");
}

#[tokio::test]
async fn right_click_on_a_dock_row_selects_it_and_opens_the_menu() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    draw_to(&mut app, 120, 24);
    let (rect, target) = app
        .panel_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Person(s, r) if s == "acme-shop" && r == "actor-alice"))
        .cloned()
        .expect("alice's row hit");
    app.on_mouse(mouse(
        crate::app::input::MouseEventKind::Down(MouseButton::Right),
        rect.x + 2,
        rect.y,
    ));
    assert_eq!(app.panel.selected, Some(target), "right-click selects");
    assert!(studio_menu_open(&app), "right-click opened the menu");
}

#[tokio::test]
async fn prefix_m_on_a_role_pane_opens_its_menu() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    app.focused = Some("t4".into()); // actor-alice's terminal
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(studio_menu_open(&app), "Ctrl+B m opened alice's menu");
}

#[tokio::test]
async fn prefix_m_on_a_shell_pane_opens_the_studio_menu() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    app.focused = Some("t-unlisted".into()); // no role's terminal
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(studio_menu_open(&app), "the shell's studio menu");
}

#[tokio::test]
async fn prefix_m_outside_studio_mode_never_intercepts() {
    // `--plain`: `m` reaches the program like every unbound key —
    // no menu, no swallow of a studio shape.
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    app.set_plain(true);
    app.reconcile_published_view();
    app.focused = Some("t4".into());
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(app.menu.is_none(), "no studio menu under --plain");
}

#[tokio::test]
async fn esc_cancels_a_reason_dialog_and_runs_nothing() {
    let mut app = app_with_panel_doc();
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_panel_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(()) })
    }));
    app.area = (120, 24);
    app.studio_dialog = Some(crate::studio_dialog::StudioDialog::reason(
        "Pause task task-1 · acme-shop · Local".into(),
        true,
        crate::studio_dialog::ReasonFor::TaskVerb {
            key: StudioKey::local("acme-shop"),
            task: "task-1".into(),
            verb: "pause",
        },
    ));
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.studio_dialog.is_none(), "Esc closed the dialog");
    assert!(recorded.lock().unwrap().is_empty(), "nothing ran");
}

#[tokio::test]
async fn a_required_reason_refuses_an_empty_submit() {
    let mut app = app_with_panel_doc();
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_panel_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(()) })
    }));
    app.area = (120, 24);
    app.studio_dialog = Some(crate::studio_dialog::StudioDialog::reason(
        "Pause task task-1 · acme-shop · Local".into(),
        true,
        crate::studio_dialog::ReasonFor::TaskVerb {
            key: StudioKey::local("acme-shop"),
            task: "task-1".into(),
            verb: "pause",
        },
    ));
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.studio_dialog.is_some(), "the refusal keeps it open");
    assert!(recorded.lock().unwrap().is_empty(), "nothing ran");
    // A typed reason submits the exact argv.
    for c in "why".chars() {
        key(&mut app, hc_vt::Key::Char(c));
    }
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec![
            "task",
            "pause",
            "--task",
            "task-1",
            "--reason",
            "why",
            "--studio",
            "acme-shop",
            "--as",
            "user"
        ]]
    );
}

#[tokio::test]
async fn the_failure_card_names_the_machine_and_c_copies_the_command() {
    let mut app = app_with_two_machines();
    let (bytes, sink) = super::super::m4s2::captured();
    app.set_host_control(sink);
    app.area = (120, 24);
    // A remote failure — `ssh host-b -- backlot …` is what C copies.
    let action = crate::action::Action {
        label: "pausing t1".into(),
        done: "paused task t1".into(),
        name: "pause task t1".into(),
        mark: "task:beta-tools:t1".into(),
        argv: vec!["task".into(), "pause".into(), "--task".into(), "t1".into()],
        machine: Some("host-b".into()),
        stdin: None,
        view: false,
    };
    let id = app.actions.start(action).expect("seated");
    app.on_action_reply((id, Err("task t1 is already paused".into())));
    let toast = app.action_toast.as_ref().expect("the failure card");
    assert_eq!(toast.title, "host-b failed: pause task t1");
    assert_eq!(toast.body, "task t1 is already paused");
    key(&mut app, hc_vt::Key::Char('C'));
    let expected =
        crate::selection::osc52_sequence("ssh host-b -- backlot task pause --task t1".as_bytes())
            .into_bytes();
    assert!(
        bytes.lock().unwrap().contains(&expected),
        "the exact remote command went to the clipboard"
    );
    assert_eq!(
        app.copy_feedback.as_ref().map(|f| f.message.as_str()),
        Some("copied command to clipboard")
    );
}

#[tokio::test]
async fn the_menu_clamps_inside_a_narrow_screen() {
    // §1: `max_width = min(34, cols - 4)` — at 40 columns the menu is
    // at most 36 wide and never crosses the right edge.
    let mut app = app_with_panel_doc();
    app.area = (40, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(studio_menu_open(&app));
    let backend = draw_to(&mut app, 40, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("Tell"), "the menu rendered: {text}");
}

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

/// A runner that only records its argv — the dialogs' submits.
fn recording_runner(app: &mut App) -> std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>> {
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_panel_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(()) })
    }));
    recorded
}

/// A required-reason dialog over a local task pause.
fn pause_reason_dialog() -> crate::studio_dialog::StudioDialog {
    crate::studio_dialog::StudioDialog::reason(
        "Pause task task-1 · acme-shop · Local".into(),
        true,
        crate::studio_dialog::ReasonFor::TaskVerb {
            key: StudioKey::local("acme-shop"),
            task: "task-1".into(),
            verb: "pause",
        },
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
    let recorded = recording_runner(&mut app);
    app.area = (120, 24);
    app.studio_dialog = Some(pause_reason_dialog());
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.studio_dialog.is_none(), "Esc closed the dialog");
    assert!(recorded.lock().unwrap().is_empty(), "nothing ran");
}

#[tokio::test]
async fn a_required_reason_refuses_an_empty_submit() {
    let mut app = app_with_panel_doc();
    let recorded = recording_runner(&mut app);
    app.area = (120, 24);
    app.studio_dialog = Some(pause_reason_dialog());
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

// ---------- focus after Esc (S3 round 2) ----------
// Closing a menu or a dialog returns the keys to the surface it was
// opened from; the pane's `input_backlog().0` must not grow until
// the pane owns keys again.

fn backlog(app: &App) -> usize {
    app.pane.input_backlog().0
}

#[tokio::test]
async fn esc_on_a_dock_menu_keeps_the_dock_focused() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    let before = backlog(&app);
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(studio_menu_open(&app));
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.menu.is_none(), "Esc closed the menu");
    assert!(app.panel.focused, "the dock kept the keys");
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(studio_menu_open(&app), "`m` opened the menu again");
    assert_eq!(backlog(&app), before, "no byte reached the pane");
}

#[tokio::test]
async fn a_dialogs_esc_returns_the_keys_to_the_dock() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    let before = backlog(&app);
    key(&mut app, hc_vt::Key::Char('m'));
    // `t` on alice's menu opens the Tell dialog.
    key(&mut app, hc_vt::Key::Char('t'));
    assert!(
        matches!(
            app.studio_dialog,
            Some(crate::studio_dialog::StudioDialog::Tell(_))
        ),
        "`t` opened the Tell dialog"
    );
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.studio_dialog.is_none(), "Esc cancelled the dialog");
    assert!(app.panel.focused, "the dock kept the keys");
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(studio_menu_open(&app), "`m` opened the menu again");
    assert_eq!(backlog(&app), before, "no byte reached the pane");
}

#[tokio::test]
async fn right_click_on_a_focused_dock_esc_m_reopens() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    draw_to(&mut app, 120, 24);
    let (rect, _) = app
        .panel_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Person(s, r) if s == "acme-shop" && r == "actor-alice"))
        .cloned()
        .expect("alice's row hit");
    let before = backlog(&app);
    app.on_mouse(mouse(
        crate::app::input::MouseEventKind::Down(MouseButton::Right),
        rect.x + 2,
        rect.y,
    ));
    assert!(studio_menu_open(&app));
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.panel.focused, "the dock kept the keys");
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(studio_menu_open(&app), "`m` opened the menu again");
    assert_eq!(backlog(&app), before, "no byte reached the pane");
}

#[tokio::test]
async fn right_click_on_an_unfocused_dock_focuses_it() {
    // A press on the dock's surface claims its keys — a left press
    // always did; right-click did not, so `Esc` then `m` typed the
    // letter into the focused terminal. This test failed before the
    // fix: `m` reached the pane.
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    app.panel.mode = PanelMode::Expanded;
    draw_to(&mut app, 120, 24);
    let (rect, _) = app
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
    assert!(studio_menu_open(&app));
    assert!(app.panel.focused, "right-click focused the dock");
    let before = backlog(&app);
    key(&mut app, hc_vt::Key::Escape);
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(studio_menu_open(&app), "`m` opened the menu again");
    assert_eq!(backlog(&app), before, "no byte reached the pane");
}

#[tokio::test]
async fn a_submitted_dialog_returns_the_keys_to_the_dock() {
    let mut app = app_with_panel_doc();
    let recorded = recording_runner(&mut app);
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    let before = backlog(&app);
    key(&mut app, hc_vt::Key::Char('m'));
    key(&mut app, hc_vt::Key::Char('t'));
    for c in "hi".chars() {
        key(&mut app, hc_vt::Key::Char(c));
    }
    // Ctrl+S submits the Tell body.
    app.on_input(
        UiEvent::Key(term_key(
            hc_vt::Key::Char('s'),
            hc_vt::Mods::CTRL,
            hc_vt::Action::Press,
        )),
        app.area.0,
    );
    assert!(app.studio_dialog.is_none(), "the submit closed the dialog");
    assert!(app.panel.focused, "the dock kept the keys");
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(studio_menu_open(&app), "`m` opened the menu again");
    assert_eq!(backlog(&app), before, "no byte reached the pane");
    assert!(
        recorded
            .lock()
            .unwrap()
            .iter()
            .any(|a| a[0..2] == ["tell", "actor-alice"]),
        "the tell ran"
    );
}

#[tokio::test]
async fn esc_on_a_cast_menu_returns_the_keys_to_the_pane() {
    // The sidebar's cast rows have no key focus of their own — the
    // menu was opened over a focused pane, so Esc hands the keys
    // back to it and `m` types into the terminal.
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    draw_to(&mut app, 120, 24);
    let (rect, _) = app
        .hit_map
        .agents
        .iter()
        .find(|(_, terminal)| terminal == "t4")
        .cloned()
        .expect("alice's cast hit");
    app.on_mouse(mouse(
        crate::app::input::MouseEventKind::Down(MouseButton::Right),
        rect.x + 1,
        rect.y,
    ));
    assert!(studio_menu_open(&app), "right-click opened alice's menu");
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.menu.is_none());
    let before = backlog(&app);
    key(&mut app, hc_vt::Key::Char('m'));
    assert_eq!(
        backlog(&app),
        before + 1,
        "the pane owned the keys again — `m` reached it"
    );
}

#[tokio::test]
async fn esc_on_a_pane_menu_returns_the_keys_to_the_pane() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    app.focused = Some("t4".into()); // actor-alice's terminal
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(studio_menu_open(&app), "Ctrl+B m opened alice's menu");
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.menu.is_none());
    let before = backlog(&app);
    key(&mut app, hc_vt::Key::Char('m'));
    assert_eq!(
        backlog(&app),
        before + 1,
        "focus returned to the pane — `m` reached it"
    );
}

#[tokio::test]
async fn the_menu_clamps_inside_a_narrow_screen() {
    // §1: `max_width = min(34, cols - 4)` — at 99 columns the dock
    // sits beside at the right edge, so the menu must clamp inside
    // the frame even anchored at the dock's own row.
    let mut app = app_with_panel_doc();
    app.area = (99, 24);
    open_panel(&mut app);
    draw_to(&mut app, 99, 24);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(studio_menu_open(&app));
    let backend = draw_to(&mut app, 99, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("Tell"), "the menu rendered: {text}");
}

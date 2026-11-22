//! R3 §1b stopped-stage truthfulness, the toast half: a mutating
//! action that lands `Ok` while its machine's stage is down stays
//! actionable — the success title keeps its green dot, the body says
//! the change is on disk, `[s] start stage` holds the card for the
//! failure card's 8 s and fires from the key, the chip and an open
//! Dock or Call Sheet alike. `stage` verbs, reads and a live stage
//! keep the one-line 3.5 s card. Neutral fixtures only (`acme-shop`,
//! `beta-tools`, `host-b`).

use super::studio_machines::two_machine_studio_app;
use super::*;
use crate::action::{DONE_HOLD, FAIL_HOLD};
use crate::app::input::{MouseButton, MouseEventKind, MouseInput, UiEvent};
use crate::studio::StudioKey;
use crate::studio_cmds;

fn local() -> StudioKey {
    StudioKey {
        machine: None,
        id: "acme-shop".into(),
    }
}

fn remote() -> StudioKey {
    StudioKey {
        machine: Some("host-b".into()),
        id: "beta-tools".into(),
    }
}

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

/// `action` seated and answered `Ok` — the toast branch sees a real
/// reply, same as the runner's.
fn ok_reply(app: &mut App, action: crate::action::Action) {
    let id = app.actions.start(action).expect("seated");
    app.on_action_reply((id, Ok(String::new())));
}

fn key(app: &mut App, k: hc_vt::Key, mods: hc_vt::Mods) {
    app.on_input(
        UiEvent::Key(term_key(k, mods, hc_vt::Action::Press)),
        app.area.0,
    );
}

fn mouse(kind: MouseEventKind, col: u16, row: u16) -> MouseInput {
    MouseInput {
        kind,
        col,
        row,
        mods: hc_vt::Mods::NONE,
        sgr_position: None,
    }
}

/// The pending `stage start` — its `machine` is the routing the
/// remote runner would see.
fn stage_start_pending(app: &App) -> Option<Option<String>> {
    app.actions
        .pending
        .iter()
        .find(|p| p.action.argv == ["stage", "start"])
        .map(|p| p.action.machine.clone())
}

#[tokio::test]
async fn a_mutation_under_a_stopped_stage_offers_the_start() {
    let mut app = two_machine_studio_app();
    drop_terminal(&mut app, "t9");
    ok_reply(&mut app, studio_cmds::priority(&local(), "task-1", "high"));
    let toast = app.action_toast.as_ref().expect("the toast");
    // The success title and dot — nothing failed, the disk just
    // holds the change unseen.
    assert!(!toast.fail);
    assert_eq!(toast.title, "priority task-1 → high");
    assert_eq!(toast.body, "recorded on disk · stage stopped");
    assert_eq!(toast.start_stage, Some(None));
    // The actionable hold: the card outlives `DONE_HOLD` and dies on
    // `FAIL_HOLD` like a failure card would.
    assert!(!app.expire_action_toast(Instant::now() + DONE_HOLD));
    assert!(app.expire_action_toast(Instant::now() + FAIL_HOLD));
}

#[tokio::test]
async fn a_remote_mutation_names_the_stopped_machine() {
    let mut app = two_machine_studio_app();
    drop_terminal(&mut app, "host-b/t9");
    ok_reply(&mut app, studio_cmds::finish(&remote(), None));
    let toast = app.action_toast.as_ref().expect("the toast");
    assert_eq!(toast.title, "host-b: finished beta-tools");
    assert_eq!(toast.body, "recorded on disk · stage stopped on host-b");
    assert_eq!(toast.start_stage, Some(Some("host-b".into())));
}

#[tokio::test]
async fn the_card_draws_the_body_and_the_chip() {
    let mut app = two_machine_studio_app();
    app.area = (120, 24);
    drop_terminal(&mut app, "t9");
    ok_reply(&mut app, studio_cmds::tell(&local(), "producer", "hi"));
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains("queued note to producer"), "{text}");
    assert!(text.contains("recorded on disk · stage stopped"), "{text}");
    assert!(text.contains("[s] start stage"), "{text}");
    assert!(text.contains("[esc] dismiss"), "{text}");
    assert!(!app.action_chip_hit.is_empty(), "the chip's hit rect");
}

#[tokio::test]
async fn s_starts_the_toast_s_machine_and_dismisses() {
    // Local first: a `tell` answer on the down stage, then `s`.
    let mut app = two_machine_studio_app();
    let (_recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);
    drop_terminal(&mut app, "t9");
    ok_reply(&mut app, studio_cmds::tell(&local(), "producer", "hi"));
    key(&mut app, hc_vt::Key::Char('s'), hc_vt::Mods::NONE);
    assert_eq!(stage_start_pending(&app), Some(None));
    assert!(app.action_toast.is_none(), "`s` dismissed the card");

    // Then host-b: the same offer routes `stage start` to the remote.
    let mut app = two_machine_studio_app();
    let (_recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);
    drop_terminal(&mut app, "host-b/t9");
    ok_reply(&mut app, studio_cmds::revise(&remote(), "s", "body".into()));
    key(&mut app, hc_vt::Key::Char('s'), hc_vt::Mods::NONE);
    assert_eq!(stage_start_pending(&app), Some(Some("host-b".into())));
    assert!(app.action_toast.is_none());
}

#[tokio::test]
async fn the_chip_click_starts_the_stage_and_dismisses() {
    let mut app = two_machine_studio_app();
    let (_recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);
    drop_terminal(&mut app, "host-b/t9");
    ok_reply(
        &mut app,
        studio_cmds::task(&remote(), "task-1", "pause", None),
    );
    draw_to(&mut app, 120, 24);
    let chip = app.action_chip_hit;
    assert!(!chip.is_empty(), "the chip drew");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        chip.x + 1,
        chip.y,
    ));
    assert_eq!(stage_start_pending(&app), Some(Some("host-b".into())));
    assert!(app.action_toast.is_none(), "the click dismissed the card");
}

#[tokio::test]
async fn s_answers_under_an_open_call_sheet_or_dock() {
    // The expanded Call Sheet owns a bare `s` — the toast's offer
    // still wins it while the card is up.
    let mut app = two_machine_studio_app();
    let (_recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);
    drop_terminal(&mut app, "host-b/t9");
    ok_reply(
        &mut app,
        studio_cmds::task(&remote(), "task-1", "pause", None),
    );
    app.hollywood.mode = crate::hollywood::LayerMode::Expanded;
    key(&mut app, hc_vt::Key::Char('s'), hc_vt::Mods::NONE);
    assert_eq!(stage_start_pending(&app), Some(Some("host-b".into())));
    assert!(app.action_toast.is_none());

    // And the focused Dock's own `s` yields to the same toast.
    let mut app = two_machine_studio_app();
    let (_recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);
    drop_terminal(&mut app, "host-b/t9");
    ok_reply(
        &mut app,
        studio_cmds::task(&remote(), "task-1", "pause", None),
    );
    key(&mut app, hc_vt::Key::Char('b'), hc_vt::Mods::CTRL);
    key(&mut app, hc_vt::Key::Char('y'), hc_vt::Mods::NONE);
    assert!(app.panel.focused, "the dock is focused");
    key(&mut app, hc_vt::Key::Char('s'), hc_vt::Mods::NONE);
    assert_eq!(stage_start_pending(&app), Some(Some("host-b".into())));
    assert!(app.action_toast.is_none());
}

#[tokio::test]
async fn s_keeps_typing_into_the_tell_line() {
    // A surface reading `s` as text owns the key — the offer waits.
    let mut app = two_machine_studio_app();
    let (_recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.area = (120, 24);
    drop_terminal(&mut app, "t9");
    ok_reply(&mut app, studio_cmds::priority(&local(), "task-1", "low"));
    app.hollywood.mode = crate::hollywood::LayerMode::Expanded;
    app.hollywood.editing = true;
    key(&mut app, hc_vt::Key::Char('s'), hc_vt::Mods::NONE);
    assert_eq!(stage_start_pending(&app), None, "the key stayed text");
    assert_eq!(app.hollywood.input.as_str(), "s");
    assert!(app.action_toast.is_some(), "the card rides on");
}

#[tokio::test]
async fn a_mutation_under_a_live_stage_keeps_the_quiet_card() {
    let mut app = two_machine_studio_app();
    ok_reply(&mut app, studio_cmds::priority(&local(), "task-1", "high"));
    let toast = app.action_toast.as_ref().expect("the toast");
    assert_eq!(toast.start_stage, None);
    assert!(toast.body.is_empty());
    assert!(app.expire_action_toast(Instant::now() + DONE_HOLD));
}

#[tokio::test]
async fn stage_verbs_and_reads_never_offer_themselves() {
    let mut app = two_machine_studio_app();
    drop_terminal(&mut app, "t9");
    for action in [
        studio_cmds::stage_stop(None),
        studio_cmds::stage_start(None),
    ] {
        ok_reply(&mut app, action);
        let toast = app.action_toast.as_ref().expect("the toast");
        assert_eq!(toast.start_stage, None, "{}", toast.title);
        assert!(toast.body.is_empty(), "{}", toast.title);
        assert!(app.expire_action_toast(Instant::now() + FAIL_HOLD));
    }
}

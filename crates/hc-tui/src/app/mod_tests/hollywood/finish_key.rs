//! The band's `f`: it arms the finish confirm only on the studio that
//! is `ready_to_finish` — a selected frame and the READY row are the
//! same studio — and stays a no-op everywhere else (a finished chip,
//! an open-task studio, the new-studio frame).

use super::*;
use crate::hollywood::Confirm;

/// Press `f` on whatever is selected.
fn press_f(app: &mut App) {
    key(app, hc_vt::Key::Char('f'));
}

/// Select `target` the way a click or the arrows do, then redraw so
/// the frame window and the band are the ones a person would press on.
fn select(app: &mut App, target: Target) {
    app.hollywood.mode = LayerMode::Expanded;
    app.hollywood.selected = Some(target);
    app.area = (120, 24);
    draw_to(app, 120, 24);
}

/// A ready studio's frame arms its finish confirm — the frame and the
/// READY row name the same studio.
#[tokio::test]
async fn f_on_a_ready_studios_frame_arms_finish() {
    let mut app = super::super::help_launcher::s5_app();
    let key = crate::studio::StudioKey::local("beta-tools");
    select(&mut app, Target::Studio(key.clone()));
    press_f(&mut app);
    assert_eq!(
        app.hollywood.confirm,
        Some(Confirm::Finish(key)),
        "the ready frame's f arms finish"
    );
}

/// A finished studio's frame is a no-op: `f` never re-arms a finish
/// the ledger already recorded.
#[tokio::test]
async fn f_on_a_finished_studios_frame_does_nothing() {
    let mut app = app_with_doc();
    let key = crate::studio::StudioKey::local("beta-tools");
    select(&mut app, Target::Studio(key));
    press_f(&mut app);
    assert_eq!(app.hollywood.confirm, None, "a finished frame never arms");
}

/// A live studio with open tasks is a no-op too: the finish command's
/// refusal, not the band, is what tells the person work is open.
#[tokio::test]
async fn f_on_a_live_studios_frame_does_nothing() {
    let mut app = app_with_doc();
    let key = crate::studio::StudioKey::local("acme-shop");
    select(&mut app, Target::Studio(key));
    press_f(&mut app);
    assert_eq!(app.hollywood.confirm, None, "an open studio never arms");
}

/// The new-studio frame and other rows stay no-ops — `f` is never a
/// create or an unrelated press.
#[tokio::test]
async fn f_on_the_new_studio_frame_does_nothing() {
    let mut app = app_with_doc();
    select(&mut app, Target::NewStudio);
    press_f(&mut app);
    assert_eq!(app.hollywood.confirm, None);
}

/// `f` on the READY row still arms exactly as before — the band's
/// existing path is untouched by the frame rule.
#[tokio::test]
async fn f_on_the_ready_row_still_arms_finish() {
    let mut app = super::super::help_launcher::s5_app();
    let key = crate::studio::StudioKey::local("beta-tools");
    select(&mut app, Target::Ready(key.clone()));
    press_f(&mut app);
    assert_eq!(app.hollywood.confirm, Some(Confirm::Finish(key)));
}

/// After a finished studio's `f` does nothing, `y` runs nothing — the
/// no-op does not leave a stale confirm a later key could fire.
#[tokio::test]
async fn a_noop_f_leaves_y_inert() {
    let mut app = app_with_doc();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    let studio_key = crate::studio::StudioKey::local("beta-tools");
    select(&mut app, Target::Studio(studio_key));
    press_f(&mut app);
    key(&mut app, hc_vt::Key::Char('y'));
    assert!(recorded.lock().unwrap().is_empty(), "y ran nothing");
}

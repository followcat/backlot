//! `Edit cast…` (step 2): the studio menu's row, the fetch → dry-run →
//! apply flow with the file's content, the "nothing changes" and refusal
//! cases shown in the dialog, and an actor gaining `Recast…`.

use std::sync::{Arc, Mutex};

use super::delete_studio::drain_dialog_fetch;
use super::lifecycle_fixtures::*;
use super::*;
use crate::studio_dialog::{CastEditStep, StudioDialog};
use crate::studio_menu::StudioAct;
use hc_vt::Key;

/// An app whose runner answers `studio cast show` with `CAST_TOML`, the
/// dry-run with `dry_reply`, and records every argv.
fn cast_app(dry_reply: Result<String, String>) -> (App, Arc<Mutex<Vec<Vec<String>>>>) {
    let mut app = lifecycle_app();
    let recorded = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_ns_run(Arc::new(move |_machine, argv, _stdin| {
        sink.lock().unwrap().push(argv.clone());
        let words: Vec<&str> = argv.iter().map(String::as_str).collect();
        let out = if words.starts_with(&["studio", "cast", "show"]) {
            Ok(CAST_TOML.to_owned())
        } else if words.starts_with(&["studio", "cast", "set"]) {
            dry_reply.clone()
        } else {
            Ok(String::new())
        };
        Box::pin(async move { out })
    }));
    (app, recorded)
}

/// The `Edit cast…` row sits on the studio menu beside the other studio
/// actions (both a live and a finished studio).
#[tokio::test]
async fn the_studio_menu_offers_edit_cast() {
    let app = lifecycle_app();
    assert!(
        labels(&studio_menu(&app, "acme-shop"))
            .iter()
            .any(|l| l == "Edit cast…"),
        "a live studio offers Edit cast…"
    );
    assert!(
        labels(&studio_menu(&app, "old-shop"))
            .iter()
            .any(|l| l == "Edit cast…"),
        "a finished studio offers Edit cast…"
    );
}

/// An actor's menu gains `Recast…` too (it keeps `Close…`).
#[tokio::test]
async fn an_actor_row_offers_recast() {
    let app = lifecycle_app();
    let alice = labels(&role_menu(&app, "actor-alice"));
    assert!(alice.iter().any(|l| l == "Recast…"), "{alice:?}");
    assert!(alice.iter().any(|l| l == "Close alice…"), "{alice:?}");
}

#[tokio::test]
async fn edit_cast_fetches_the_effective_cast() {
    let (mut app, recorded) = cast_app(Ok(CAST_DIFF.to_owned()));
    app.run_studio_act(&studio_menu(&app, "acme-shop"), StudioAct::EditCast);
    assert!(matches!(app.studio_dialog, Some(StudioDialog::CastEdit(_))));
    // The show fetch runs through the runner with the studio's flags.
    let argv = recorded.lock().unwrap().first().cloned().unwrap();
    assert_eq!(
        argv,
        super::lifecycle_fixtures::argv(&[
            "studio",
            "cast",
            "show",
            "--studio",
            "acme-shop",
            "--as",
            "user"
        ])
    );
}

/// The whole flow: show lands the cast and opens the editor; the edited
/// file's dry-run shows a diff; Enter records `studio cast set` with the
/// file's content on stdin.
#[tokio::test]
async fn edit_cast_checks_the_change_and_applies_it() {
    let (mut app, recorded) = cast_app(Ok(CAST_DIFF.to_owned()));
    app.run_studio_act(&studio_menu(&app, "acme-shop"), StudioAct::EditCast);
    // Deliver the show reply through the action tracker.
    recv_action(&mut app).await;
    let Some(StudioDialog::CastEdit(d)) = app.studio_dialog.as_ref() else {
        panic!("the cast dialog");
    };
    assert_eq!(
        d.step,
        CastEditStep::Editing,
        "the cast opened in the editor"
    );

    // The editor wrote an edited file; simulate its close with edited
    // text (the editor pops up for real only with an operations channel).
    app.cast_edit_edited("rework_cap = 5\n");
    drain_dialog_fetch(&mut app).await;
    let Some(StudioDialog::CastEdit(d)) = app.studio_dialog.as_ref() else {
        panic!("the cast dialog after the check");
    };
    assert_eq!(d.step, CastEditStep::Diff);
    assert!(d.ready, "a diff readies the apply");
    assert!(d.diff.contains("candidate + actor-b"), "{}", d.diff);

    // Enter records it.
    key(&mut app, Key::Enter);
    assert!(app.studio_dialog.is_none());
    let sent = recorded.lock().unwrap().last().cloned().unwrap();
    assert!(
        sent.starts_with(&super::lifecycle_fixtures::argv(&[
            "studio", "cast", "set", "--file", "-"
        ])),
        "{sent:?}"
    );
    assert!(
        !sent.iter().any(|a| a == "--dry-run"),
        "the apply, not a re-check"
    );
}

/// The dry run's own argv carries `--dry-run` and the edited content on
/// stdin; nothing is applied on Enter while only the check has run.
#[tokio::test]
async fn edit_cast_dry_run_uses_stdin_and_records_nothing_on_enter() {
    let (mut app, recorded) = cast_app(Ok(CAST_DIFF.to_owned()));
    app.run_studio_act(&studio_menu(&app, "acme-shop"), StudioAct::EditCast);
    recv_action(&mut app).await;
    app.cast_edit_edited("rework_cap = 5\n");
    drain_dialog_fetch(&mut app).await;
    let sent = recorded.lock().unwrap().last().cloned().unwrap();
    assert!(sent.iter().any(|a| a == "--dry-run"), "{sent:?}");
    assert!(sent.windows(2).any(|w| w == ["--file", "-"]), "{sent:?}");
}

/// A refused dry run (a runner the machine has not got, an unchanged
/// file) shows in the dialog with its reason; Enter does nothing.
#[tokio::test]
async fn a_refused_change_shows_its_reason() {
    let (mut app, recorded) = cast_app(Err(
        "nothing changes: the file says what the studio already plays".into(),
    ));
    app.run_studio_act(&studio_menu(&app, "acme-shop"), StudioAct::EditCast);
    recv_action(&mut app).await;
    app.cast_edit_edited("rework_cap = 2\n");
    drain_dialog_fetch(&mut app).await;
    let Some(StudioDialog::CastEdit(d)) = app.studio_dialog.as_ref() else {
        panic!("the cast dialog");
    };
    assert!(!d.ready, "a refusal readies nothing");
    assert!(
        d.error.as_deref().unwrap().contains("nothing changes"),
        "{:?}",
        d.error
    );
    let before = recorded.lock().unwrap().len();
    key(&mut app, Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().len(),
        before,
        "Enter on a refused change sends nothing"
    );
    assert!(
        matches!(app.studio_dialog, Some(StudioDialog::CastEdit(_))),
        "the dialog stays open on a refusal"
    );
}

/// A failed fetch shows in the dialog; the studio is not applied.
#[tokio::test]
async fn a_failed_fetch_shows_in_the_dialog() {
    let (mut app, _recorded) = cast_app(Ok(CAST_DIFF.to_owned()));
    app.set_ns_run(Arc::new(|_, _, _| {
        Box::pin(async { Err("no studio 'acme-shop'".into()) })
    }));
    app.run_studio_act(&studio_menu(&app, "acme-shop"), StudioAct::EditCast);
    recv_action(&mut app).await;
    let Some(StudioDialog::CastEdit(d)) = app.studio_dialog.as_ref() else {
        panic!("the cast dialog");
    };
    assert!(
        d.error.as_deref().unwrap().contains("no studio"),
        "{:?}",
        d.error
    );
}

/// The dialog renders its title, the step line and the diff, and the
/// apply button only when a change is ready.
#[test]
fn the_dialog_renders_title_step_and_diff() {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    // Ready: the diff shows.
    let mut d =
        crate::studio_dialog::CastEditDialog::new(&crate::studio::StudioKey::local("acme-shop"));
    d.dry_run(Ok("candidate + actor-b (claude/m2/high (worktree))".into()));
    let text = render(&d);
    assert!(text.contains("Edit cast · acme-shop"), "{text}");
    assert!(text.contains("the change"), "{text}");
    assert!(text.contains("candidate + actor-b"), "{text}");
    assert!(text.contains("apply"), "{text}");
    // Refused: the reason shows, no diff.
    let mut d =
        crate::studio_dialog::CastEditDialog::new(&crate::studio::StudioKey::local("acme-shop"));
    d.dry_run(Err("limits.fake = 2 leaves no slot".into()));
    let text = render(&d);
    assert!(text.contains("limits.fake = 2"), "{text}");
    let _ = (Buffer::empty(Rect::new(0, 0, 1, 1)), Rect::new(0, 0, 1, 1));
}

/// Receive the one action reply the last `run_action` queued and hand it
/// to the tracker — the show fetch, or an apply.
async fn recv_action(app: &mut App) {
    let reply = app.action_rx.recv().await.expect("one action reply");
    app.on_action_reply(reply);
}

/// An actor's `Recast…` opens the same picker and runs `recast` on the
/// actor key — the identity prefilled from the actor's current one.
#[tokio::test]
async fn an_actor_recast_sends_the_actor_key() {
    let mut app = lifecycle_app();
    let recorded = scripted(&mut app);
    app.run_studio_act(&role_menu(&app, "actor-alice"), StudioAct::Recast);
    drain_dialog_fetch(&mut app).await;
    for _ in 0..4 {
        key(&mut app, Key::Enter);
    }
    let sent = recorded.lock().unwrap().last().cloned().unwrap();
    assert_eq!(
        sent,
        super::lifecycle_fixtures::argv(&[
            "recast",
            "--person",
            "actor-alice",
            "--runner",
            "claude",
            "--model",
            "m1",
            "--effort",
            "medium",
            "--studio",
            "acme-shop",
            "--as",
            "user"
        ])
    );
}

/// Enter while the fetch is loading does nothing — no argv runs.
#[tokio::test]
async fn edit_cast_enter_waits_while_loading() {
    let (mut app, recorded) = cast_app(Ok(CAST_DIFF.to_owned()));
    app.run_studio_act(&studio_menu(&app, "acme-shop"), StudioAct::EditCast);
    let before = recorded.lock().unwrap().len();
    key(&mut app, Key::Enter);
    assert!(
        matches!(app.studio_dialog, Some(StudioDialog::CastEdit(_))),
        "the dialog stays while loading"
    );
    assert_eq!(
        recorded.lock().unwrap().len(),
        before,
        "Enter on a loading dialog sends nothing"
    );
}

/// A cast card renders its label like any other card.
#[test]
fn a_cast_card_label_renders() {
    let action = backlot_model::hollywood::CardAction::Cast {
        studio: "acme-shop".into(),
        cast: "/home/user/acme-shop/cast.toml".into(),
        snapshot: None,
    };
    assert_eq!(action.label(), "cast acme-shop ← cast.toml");
    let restart = backlot_model::hollywood::CardAction::Restart {
        studio: "acme-shop".into(),
        person: "director".into(),
        fresh: false,
    };
    assert_eq!(restart.label(), "restart director");
}

/// Render one cast dialog and flatten the buffer to text.
fn render(d: &crate::studio_dialog::CastEditDialog) -> String {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    let band = Rect::new(0, 0, 100, 30);
    let mut buf = Buffer::empty(band);
    let palette = crate::chrome::palette::Palette::backlot();
    let dialog = StudioDialog::CastEdit(d.clone());
    crate::chrome::studio_dialogs::render(&mut buf, band, &dialog, &palette);
    let mut out = String::new();
    for y in 0..band.height {
        for x in 0..band.width {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

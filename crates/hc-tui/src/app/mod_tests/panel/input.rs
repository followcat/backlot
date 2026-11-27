//! The `›` line's keys: `i`/`/`/`⏎`/a click arm `Compose` on it
//! (the explicit `editing` latch does the rest), a plain line is a
//! `tell` to the producer, a `/verb` dispatches through the
//! studio-menu arm — `/stop` arms the same confirm `m` would — and
//! an armed `finish` prompt's letters answer it instead of landing
//! in the draft. `⏎` sends back to `Resting` (R3-2's dock rule):
//! the buffer clears, the selection stays on `Target::Input`, and
//! `j`/`k`/`m`/`Esc` act on the dock at once. Cross-studio draft
//! isolation, paste/IME routing, Home/End and the finished-studio
//! reopen row live in `compose`.
use super::*;

#[tokio::test]
async fn i_lands_the_selection_on_the_input_row_and_types() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('i'));
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    assert!(app.panel_editing());
    // Letters are text on the line, not dock keys — `j` moves
    // nothing while the line owns the keys.
    key(&mut app, hc_vt::Key::Char('j'));
    assert_eq!(draft(&app, "acme-shop"), "j");
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
}

#[tokio::test]
async fn a_plain_line_tells_the_producer_and_the_row_rests() {
    let mut app = app_with_panel_doc();
    let (recorded, run) = runner();
    app.set_panel_runner(run);
    app.area = (120, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('i'));
    for ch in "check the queue".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec![
            "tell".to_string(),
            "producer".into(),
            "--text".into(),
            "check the queue".into(),
            "--studio".into(),
            "acme-shop".into(),
            "--as".into(),
            "user".into(),
        ]]
    );
    // R3-2's dock rule: the send cleared the draft and the row
    // dropped to `Resting` — `j`/`k`/`m`/`Esc` work at once while
    // the selection keeps its `›` place.
    assert_eq!(draft(&app, "acme-shop"), "");
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    assert!(!app.panel_editing());
}

#[tokio::test]
async fn esc_rests_keeping_the_draft_and_a_second_esc_unfocuses() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('i'));
    key(&mut app, hc_vt::Key::Char('x'));
    // First Esc: `Compose` → `Resting` on the same row, draft kept.
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.panel_editing());
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    assert_eq!(draft(&app, "acme-shop"), "x");
    // Second Esc: the dock's usual leave — focus hands back.
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.panel.focused);
    // The parked draft survived the whole round-trip.
    assert_eq!(draft(&app, "acme-shop"), "x");
}

#[tokio::test]
async fn up_rests_and_steps_off_the_line() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('i'));
    key(&mut app, hc_vt::Key::Char('x'));
    // In Compose, `↑` stays on the `›` line (history); `Esc` rests, then `↑` steps off.
    key(&mut app, hc_vt::Key::ArrowUp);
    assert!(app.panel_editing());
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.panel_editing());
    key(&mut app, hc_vt::Key::ArrowUp);
    assert!(!app.panel_editing());
    assert!(matches!(
        app.panel.selected,
        Some(
            Target::Person(..)
                | Target::Studio(..)
                | Target::Task(..)
                | Target::Fold(..)
                | Target::ClosedFold(..)
        )
    ));
    assert_eq!(draft(&app, "acme-shop"), "x", "the draft parked");
    // ↓ steps back onto the line — still `Resting`: walking onto a
    // `›` row never arms `Compose` on its own.
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    assert!(!app.panel_editing());
    assert_eq!(draft(&app, "acme-shop"), "x");
    // `Enter` on the resting row re-arms `Compose` with the draft.
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.panel_editing());
    assert_eq!(draft(&app, "acme-shop"), "x");
}

#[tokio::test]
async fn i_on_a_person_seeds_their_tell() {
    let mut app = app_with_panel_doc();
    let (recorded, run) = runner();
    app.set_panel_runner(run);
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    key(&mut app, hc_vt::Key::Char('i'));
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    assert_eq!(
        app.focus.workspace,
        crate::focus::WorkspaceRegion::Lot(crate::focus::LotLevel::Compose {
            studio: "acme-shop".into(),
            target: crate::focus::TellTarget::Role("actor-alice".into()),
        })
    );
    assert_eq!(draft(&app, "acme-shop"), "");
    // The press pre-targeted `actor-alice` — finishing the line sends
    // `tell actor-alice` without cluttering the input buffer.
    for ch in "ship it".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec![
            "tell".to_string(),
            "actor-alice".into(),
            "--text".into(),
            "ship it".into(),
            "--studio".into(),
            "acme-shop".into(),
            "--as".into(),
            "user".into(),
        ]]
    );
}

#[tokio::test]
async fn slash_stop_arms_the_same_confirm_the_menu_arms() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('/'));
    assert_eq!(draft(&app, "acme-shop"), "/");
    for ch in "stop".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    // The deterministic verb opened `m`'s confirm — no LLM, no
    // action run yet (the dialog decides). It is the machine-wide
    // wording the machine menu's own row uses.
    let Some(crate::studio_dialog::StudioDialog::Confirm(d)) = &app.studio_dialog else {
        panic!("the /stop verb arms the confirm");
    };
    assert_eq!(d.title, "Stop the stage · Local");
    assert!(
        d.body
            .contains("Every studio on this machine stops advancing"),
        "{}",
        d.body
    );
}

#[tokio::test]
async fn an_unknown_verb_notices_with_the_verb_list() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('/'));
    for ch in "bogus".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.notice.is_some(), "a bad verb notices");
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains("/bogus"), "{text}");
    assert!(text.contains("/stop"), "{text}");
}

#[tokio::test]
async fn a_mistyped_leading_letter_verb_never_sends() {
    let mut app = app_with_panel_doc();
    let (recorded, run) = runner();
    app.set_panel_runner(run);
    app.area = (120, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('i'));
    // The fat-fingered `i/tell producer hi` a resting `i` left in
    // the buffer — rejected, draft kept, no producer tell.
    for ch in "i/tell producer hi".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    assert!(recorded.lock().unwrap().is_empty(), "no tell ran");
    assert_eq!(draft(&app, "acme-shop"), "i/tell producer hi");
    assert!(app.panel_editing(), "the line keeps Compose for the fix");
    assert!(app.notice.is_some(), "the guard explained itself");
}

#[tokio::test]
async fn tab_completes_a_slash_verb_prefix() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('/'));
    for ch in "sto".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(draft(&app, "acme-shop"), "/stop ");
}

#[tokio::test]
async fn an_armed_finish_prompt_answers_instead_of_drafting() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('i'));
    // The `finish <id>? [y/n]` the `f` key arms outranks the line's
    // edit — `y` decides, it never lands as text.
    app.panel.finish_confirm = Some("acme-shop".into());
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(draft(&app, "acme-shop"), "");
}

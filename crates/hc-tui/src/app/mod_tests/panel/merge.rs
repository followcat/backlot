//! The merge's panel-vs-upstream decisions: the chord still fires from
//! normal mode, copy mode owns the chord's keys while a search prompt
//! is open, a focused panel eats image pastes and host text pastes
//! ahead of an open popup, and the panel's tick still runs on a tick
//! where the selection also ticks.
use super::*;
use crate::app::copy_mode::{CopyModeState, CopySearchPrompt};
use crate::copy_mode::{CopySearchDirection, PaneTextPoint};
use crate::popup_size::PopupSize;

fn paste(app: &mut App, text: &[u8]) {
    app.on_input(UiEvent::Paste(text.to_vec()), app.area.0);
}

#[tokio::test]
async fn the_chord_toggles_the_panel_from_normal_mode() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.panel.mode, PanelMode::Closed);
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(app.panel.mode, PanelMode::Expanded);
    assert!(app.panel.focused);
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(app.panel.mode, PanelMode::Closed);
}

#[tokio::test]
async fn a_copy_mode_search_prompt_takes_the_chords_keys() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    app.mode = Mode::Copy;
    app.copy_mode = Some(CopyModeState {
        pane_id: "t1".into(),
        content_revision: 0,
        geometry: (80, 24),
        alternate_screen_active: false,
        cursor: PaneTextPoint { row: 0, col: 0 },
        offset_from_bottom: 0,
        max_offset_from_bottom: 0,
        entry_offset_from_bottom: 0,
        selection: None,
        search_prompt: Some(CopySearchPrompt {
            direction: CopySearchDirection::Forward,
            query: crate::chrome::text_editor::TextEditor::new("", false),
        }),
        search_query: String::new(),
        search_direction: None,
        search_matches: Vec::new(),
        search_total: 0,
        search_current: None,
        search_current_global: None,
    });
    // Prefix then the panel key: with the prompt open both go to the
    // query — no toggle, no copy-mode exit.
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(app.mode, Mode::Copy, "the prompt owned the chord");
    assert_eq!(app.panel.mode, PanelMode::Closed);
    let query = app
        .copy_mode
        .as_ref()
        .and_then(|copy_mode| copy_mode.search_prompt.as_ref())
        .map(|prompt| prompt.query.as_str().to_owned())
        .unwrap_or_default();
    assert_eq!(
        query, "y",
        "the chord tail reached the prompt, got {query:?}"
    );
    assert_eq!(app.pane.input_backlog().0, 0, "keys leaked to the pane");
}

#[tokio::test]
async fn a_focused_panel_eats_an_image_paste() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    app.set_image_paste_context(true, false);
    app.image_paste_reader = Box::new(|| {
        crate::clipboard::ClipboardRead::Image(crate::clipboard::ClipboardImage {
            bytes: b"\x89PNG".to_vec(),
            extension: "png",
        })
    });
    open_panel(&mut app);
    // An image-only bracketed paste and a dropped image path both have
    // no target while the panel owns input — and no text field to take
    // the bytes either.
    paste(&mut app, &[]);
    paste(&mut app, b"/home/user/shot.png");
    assert_eq!(app.pending_actions, 0, "an upload started");
    assert_eq!(app.pane.input_backlog().0, 0, "paste bytes leaked");
    assert!(app.panel.focused);
}

#[tokio::test]
async fn a_focused_panel_eats_a_host_paste_ahead_of_an_open_popup() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    // Attach openers park forever — no wire in a unit test.
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    open_panel(&mut app);
    // A popup terminal in the inventory keeps the spawned popup pane
    // able to queue input, so a delivered paste would be visible.
    let mut popup_term = crate::sidebar::fixture::term("t10");
    popup_term.created_ms = 10;
    popup_term.labels.insert("hc-tui.popup".into(), "1".into());
    let mut terminals: Vec<_> = app.view.inventory.terminals().values().cloned().collect();
    terminals.push(popup_term);
    app.view.inventory.replace(3, terminals);
    app.popup_pending = Some(crate::app::popup::PopupPending {
        width: Some(PopupSize::Percent(80)),
        height: Some(PopupSize::Percent(80)),
    });
    app.on_popup_spawned("t10".into());
    assert!(app.popup.is_some());
    assert!(app.panel_focused());
    // The paste arm reaches the focused panel before upstream's
    // `popup_paste` — the panel has no text field, so it eats it.
    paste(&mut app, b"host text");
    assert!(
        !app.popup.as_ref().unwrap().pane.pending_has_paste(),
        "the focused panel eats the paste even with a popup open"
    );
    assert_eq!(app.pane.input_backlog().0, 0, "paste bytes leaked");
    // Release panel focus and the same paste reaches the popup as a
    // semantic Paste — upstream's arm is intact behind the panel's.
    app.panel.focused = false;
    paste(&mut app, b"host text");
    assert!(app.popup.as_ref().unwrap().pane.pending_has_paste());
}

#[tokio::test]
async fn the_panel_tick_runs_when_the_selection_also_ticks() {
    let mut app = app_with_panel_doc();
    let now = Instant::now();
    app.panel_anim_deadline = Some(now);
    app.selection_highlight_clear_deadline = Some(now);
    // The runtime calls tick_panel unconditionally, then the selection
    // pair — mirror that order here.
    app.tick_panel(now);
    let repaint = app.tick_selection(now) | app.tick_copy_feedback(now);
    assert_eq!(app.panel.spin, 1, "the spin deadline fired");
    assert_eq!(app.panel_anim_deadline, None);
    assert!(repaint, "the highlight clear repaints");
    assert!(app.selection.is_none());
    assert_eq!(app.selection_highlight_clear_deadline, None);
}

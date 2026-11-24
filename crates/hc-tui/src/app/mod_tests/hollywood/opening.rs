//! The expanded Call Sheet over the opening card (U7): the band floats
//! over the one-time page instead of hiding behind it, while the
//! collapsed strip still waits for the card to leave. Esc ordering:
//! the visible band collapses first, the next Esc dismisses the page.
use super::*;

/// A studio-less start with `[hollywood]` configured: the card is up,
/// its face the lot pill, the layer collapsed.
fn page_app() -> App {
    let mut app = app_with_empty_doc();
    app.set_hollywood(Ok(Some(cfg(None))), std::path::Path::new("config.toml"));
    app.area = (120, 24);
    app
}

#[tokio::test]
async fn alt_y_draws_the_expanded_band_over_the_opening_page() {
    let mut app = page_app();
    assert!(app.opening_visible(), "a studio-less start shows the card");
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains("New studio"), "{text}");
    assert!(
        !text.contains("H O L L Y W O O D"),
        "the collapsed strip stays off the card: {text}"
    );

    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    assert!(
        app.opening_visible(),
        "the card is a page, not a mode: expanding never dismisses it"
    );
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(
        text.contains("H O L L Y W O O D"),
        "the band draws over the card: {text}"
    );
    assert!(
        !app.hollywood_hits.area.is_empty(),
        "the band owns its hit rect"
    );
}

#[tokio::test]
async fn esc_rests_then_collapses_the_band_then_dismisses_the_page() {
    let mut app = page_app();
    alt_y(&mut app);
    draw_to(&mut app, 120, 24);

    // A calm sheet opens in `Compose` — the first Esc rests the `›`
    // line (R3-2), the second collapses the band.
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.hollywood.editing, "the first Esc rests the line");
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(
        app.hollywood.mode,
        LayerMode::Collapsed,
        "a resting line collapses on the next Esc"
    );
    assert!(app.opening_visible(), "the page survives the band's Esc");
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(!text.contains("H O L L Y W O O D"), "{text}");
    assert!(
        text.contains("New studio"),
        "the card is back in front: {text}"
    );

    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.opening_visible(), "the next Esc dismisses the page");
}

#[tokio::test]
async fn the_cards_h_opens_the_tell_line_over_the_page() {
    let mut app = page_app();
    key(&mut app, hc_vt::Key::Char('h'));
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    assert!(app.hollywood.editing, "the line is taking text");
    assert!(app.opening_visible(), "the page stays under the band");
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains("H O L L Y W O O D"), "{text}");
}

/// S1: expanding with `[hollywood]` set always opens Compose on `Target::Input`
/// even when waiting cards exist (which stay visible above).
#[tokio::test]
async fn a_calm_sheet_opens_editing_a_waiting_one_does_not() {
    let mut app = page_app();
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    assert!(app.hollywood.editing, "nothing waits — the dialog opens");
    key(&mut app, hc_vt::Key::Char('h'));
    assert_eq!(app.hollywood.input.as_str(), "h");

    let mut busy = app_with_hollywood();
    alt_y(&mut busy);
    assert_eq!(busy.hollywood.mode, LayerMode::Expanded);
    assert!(
        busy.hollywood.editing,
        "S1: Alt+Y lands ready to type even with waits"
    );
    assert_eq!(busy.hollywood.selected, Some(Target::Input));
    assert_eq!(busy.hollywood.input.as_str(), "");
}

/// In Compose, `↑` stays on the `›` line (recalling history) and `Esc` rests on
/// `Input` keeping the draft; from Resting, `↑` walks rows, `↓` returns to
/// `Input` Resting, `Enter` re-arms Compose, and `Esc` collapses (S1).
#[tokio::test]
async fn up_rests_a_row_up_and_esc_rest_then_collapse() {
    let mut app = page_app();
    alt_y(&mut app);
    assert!(app.hollywood.editing);
    key(&mut app, hc_vt::Key::Char('h'));
    // In Compose, `↑` stays on the line without leaving Compose.
    key(&mut app, hc_vt::Key::ArrowUp);
    assert!(app.hollywood.editing);
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert_eq!(app.hollywood.input.as_str(), "h");
    // `Esc` rests on `Input`, keeping the draft; then `↑` steps up a row.
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.hollywood.editing);
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert_eq!(app.hollywood.input.as_str(), "h");
    key(&mut app, hc_vt::Key::ArrowUp);
    assert!(!app.hollywood.editing);
    assert!(app.hollywood.selected != Some(Target::Input));
    assert_eq!(app.hollywood.input.as_str(), "h");
    // `↓` steps back onto the line — `Resting`, draft still parked.
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.hollywood.selected, Some(Target::Input));
    assert!(!app.hollywood.editing);
    assert_eq!(app.hollywood.input.as_str(), "h");
    // `Enter` on the resting row re-arms `Compose` with the draft.
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.hollywood.editing);
    assert_eq!(app.hollywood.input.as_str(), "h");
    // The compose `Esc` rests on the row — the sheet stays open.
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.hollywood.editing);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    assert_eq!(app.hollywood.input.as_str(), "h");
    // The resting `Esc` collapses the sheet.
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    assert_eq!(app.hollywood.input.as_str(), "h", "the draft survives");
}

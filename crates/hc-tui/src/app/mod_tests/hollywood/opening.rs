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
async fn esc_collapses_the_band_first_then_dismisses_the_page() {
    let mut app = page_app();
    alt_y(&mut app);
    draw_to(&mut app, 120, 24);

    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(
        app.hollywood.mode,
        LayerMode::Collapsed,
        "the visible band takes the first Esc"
    );
    assert!(app.opening_visible(), "the page survives the first Esc");
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

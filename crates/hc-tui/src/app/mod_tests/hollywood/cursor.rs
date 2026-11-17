//! The pane's hardware cursor never shines through the expanded bar
//! (U7 fix 2). A bar is opaque for cells, and it must be opaque for the
//! cursor too: it is drawn over the panes, so a cursor underneath it
//! would sit in the middle of a frame or the input line.
use super::*;

/// A focused pane whose cursor sits a few rows down its surface — inside
/// the band while the bar is expanded at this size.
fn app_with_pane_cursor() -> App {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    let mut vt = hc_vt::Vt::new(80, 20, 0);
    vt.feed(b"line one\r\nline two\r\nline three");
    app.pane.install_vt(vt);
    app
}

#[tokio::test]
async fn the_expanded_bar_hides_the_panes_cursor_under_it() {
    let mut app = app_with_pane_cursor();
    // The strip covers no pane cell: the cursor stays where it is.
    app.hollywood.mode = LayerMode::Collapsed;
    draw_to(&mut app, 120, 24);
    assert!(
        app.resolve_host_cursor().visible,
        "a collapsed bar leaves the pane's cursor alone"
    );

    // Expanded, the band covers the cursor's cell: it is not shown.
    app.hollywood.mode = LayerMode::Expanded;
    draw_to(&mut app, 120, 24);
    assert!(
        !app.hollywood_hits.area.is_empty(),
        "the band is drawn over the cursor's cell"
    );
    assert!(
        !app.resolve_host_cursor().visible,
        "the pane's cursor is hidden under the band"
    );

    // While the `tell hollywood` line edits, the cursor is on the input.
    app.hollywood.editing = true;
    draw_to(&mut app, 120, 24);
    let input = app.hollywood_hits.input;
    assert!(!input.is_empty(), "the input line is drawn");
    let host = app.resolve_host_cursor();
    assert!(host.visible, "the input line shows the cursor");
    assert!(
        input.contains(ratatui::layout::Position::new(host.x, host.y)),
        "the cursor sits on the input line, not the pane: {host:?} in {input:?}"
    );
}

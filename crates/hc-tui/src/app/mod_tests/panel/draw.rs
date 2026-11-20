//! The panel over the damage-aware draw: open, close, width changes
//! and resizes must repaint exactly what moved — no pane paint may
//! stay under the panel and no panel paint may stay in the panes.
//! Each app carries a live VT behind `t1` so the pane's render cache
//! is real and the geometry changes are what invalidate it.
//!
//! The pane's attach is parked forever, so the `Reconnecting` card
//! legitimately overlays frame rows 1-4 — markers live under it.
use super::*;

/// One active pane (`t1`) with `PANE-ALPHA` on its eighth row — under
/// the reconnect card's rows.
fn app_with_live_pane() -> App {
    let mut app = app_with_panel_doc();
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    app.reconcile_published_view();
    app.focus_pane("t1".into());
    let mut vt = hc_vt::Vt::new(90, 22, 0);
    vt.feed(b"\r\n\r\n\r\n\r\n\r\n\r\n\r\nPANE-ALPHA");
    app.pane.install_vt(vt);
    app
}

/// The rows' text across columns `[left, right)`.
fn columns_text(b: &TestBackend, left: u16, right: u16) -> String {
    let (w, h) = (b.size().unwrap().width, b.size().unwrap().height);
    let buf = b.buffer();
    (0..h)
        .map(|y| {
            (left..right.min(w))
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn open_close_open_repaints_both_regions() {
    let mut app = app_with_live_pane();
    app.area = (120, 24);
    let closed = draw_to(&mut app, 120, 24);
    let text = screen_lines(&closed).join("\n");
    assert!(text.contains("PANE-ALPHA"), "{text}");
    // ` lot` (not bare `lot`: the sidebar's machine line names `backlot`).
    assert!(!text.contains(" lot"), "{text}");

    open_panel(&mut app);
    let open = draw_to(&mut app, 120, 24);
    let text = screen_lines(&open).join("\n");
    assert!(text.contains(" lot"), "{text}");
    assert!(text.contains("alice · task-1"), "{text}");
    assert!(text.contains("PANE-ALPHA"), "{text}");
    // The right 26 columns are all panel: no pane paint remains there.
    let right = columns_text(&open, 84, 120);
    assert!(right.contains("lot"), "{right}");
    assert!(!right.contains("PANE-ALPHA"), "{right}");

    app.panel_close();
    let closed_again = draw_to(&mut app, 120, 24);
    assert_eq!(
        closed.buffer(),
        closed_again.buffer(),
        "closing must restore the pre-panel frame cell for cell"
    );

    open_panel(&mut app);
    let reopened = draw_to(&mut app, 120, 24);
    assert_eq!(
        open.buffer(),
        reopened.buffer(),
        "reopening must reproduce the first open frame cell for cell"
    );
}

#[tokio::test]
async fn a_pane_update_while_the_panel_is_open_repaints_cleanly() {
    let mut app = app_with_live_pane();
    app.area = (120, 24);
    open_panel(&mut app);
    draw_to(&mut app, 120, 24);
    // A dirty row in the shrunken pane repaints there; the panel's
    // columns stay the panel's through the partial rebuild.
    app.pane.vt().unwrap().feed(b"\r\nPANE-BRAVO");
    let open = draw_to(&mut app, 120, 24);
    let text = screen_lines(&open).join("\n");
    assert!(text.contains("PANE-ALPHA"), "{text}");
    assert!(text.contains("PANE-BRAVO"), "{text}");
    let right = columns_text(&open, 84, 120);
    assert!(right.contains("lot"), "{right}");
    assert!(!right.contains("PANE"), "{right}");
}

#[tokio::test]
async fn a_resize_moves_the_panel_between_beside_and_overlay() {
    let mut app = app_with_live_pane();
    app.area = (120, 24);
    open_panel(&mut app);
    draw_to(&mut app, 120, 24);
    // At 80 the sidebar + the 28-column dock can't leave 44 pane
    // columns: the expanded dock resolves to the collapsed strip —
    // the right 16 columns carry the strip, the pane keeps its
    // surface (S2 §1's fallback).
    app.area = (80, 24);
    let wide = draw_to(&mut app, 80, 24);
    let text = screen_lines(&wide).join("\n");
    assert!(text.contains("PANE-ALPHA"), "{text}");
    let right = columns_text(&wide, 64, 80);
    assert!(right.contains("lot"), "{right}");
    assert!(!right.contains("PANE-ALPHA"), "{right}");
    // Back at 120 the expanded column returns.
    app.area = (120, 24);
    let back = draw_to(&mut app, 120, 24);
    let right = columns_text(&back, 84, 120);
    assert!(right.contains("lot"), "{right}");
    assert!(!right.contains("PANE-ALPHA"), "{right}");
}

#[tokio::test]
async fn the_collapsed_hollywood_strip_clears_the_panel_title() {
    let mut app = app_with_live_pane();
    app.area = (160, 40);
    app.hollywood.mode = crate::hollywood::LayerMode::Collapsed;
    open_panel(&mut app);
    let drawn = draw_to(&mut app, 160, 40);
    // The strip's rect ends where the panel's column begins — it may
    // not share a cell with the panel's title row.
    let strip = app.hollywood_hits.area;
    let title = app.panel_hits.title;
    assert!(
        strip.is_empty() || strip.x + strip.width <= title.x,
        "strip {strip:?} overlaps the panel title {title:?}"
    );
    // And the panel's title row reads its own text, not the strip's.
    let top = columns_text(&drawn, title.x, title.x + title.width);
    let first = top.lines().next().unwrap_or_default().to_string();
    assert!(first.contains("lot"), "{first}");
    assert!(!first.contains("hollywood"), "{first}");
}

#[tokio::test]
async fn the_collapsed_strip_draws_without_touching_the_panes() {
    let mut app = app_with_live_pane();
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.mode = PanelMode::Collapsed;
    let collapsed = draw_to(&mut app, 120, 24);
    // The strip is 16 columns — its rows clip to it and the panes own
    // everything left of it.
    let strip = columns_text(&collapsed, 104, 120);
    assert!(strip.contains("lot"), "{strip}");
    assert!(!strip.contains("PANE-ALPHA"), "{strip}");
    let panes = columns_text(&collapsed, 0, 104);
    assert!(!panes.contains(" lot"), "{panes}");
    // Expanding again puts the full column back.
    app.panel.mode = PanelMode::Expanded;
    let expanded = draw_to(&mut app, 120, 24);
    let right = columns_text(&expanded, 84, 120);
    assert!(right.contains("alice · task-1"), "{right}");
}

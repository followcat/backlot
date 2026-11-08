//! M5: mouse text selection in panes — herdr's press/drag/word gestures,
//! edge autoscroll and OSC 52 copy over the local displayed VT.
use super::*;
use hc_vt::{Action, Key, Mods};
use input::{MouseButton, MouseEventKind, MouseInput};
use std::sync::Mutex;
use std::time::Duration;

fn pending(app: &mut App) {
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
}

/// A one-pane app whose attach is a local VT already drawn once, so the
/// hit map carries real inner rects and scroll metrics.
fn selection_app(vt: hc_vt::Vt) -> App {
    let mut app = app_with(&["t1"]);
    pending(&mut app);
    app.reconcile_published_view();
    app.focus_pane("t1".into());
    app.pane.install_vt(vt);
    draw_to(&mut app, 80, 24);
    app
}

fn capture_host(app: &mut App) -> Arc<Mutex<Vec<u8>>> {
    let out = Arc::new(Mutex::new(Vec::new()));
    let sink = out.clone();
    app.host_control = Box::new(move |bytes| {
        sink.lock().unwrap().extend_from_slice(bytes);
        Ok(())
    });
    out
}

fn mouse(kind: MouseEventKind, col: u16, row: u16) -> MouseInput {
    MouseInput {
        kind,
        col,
        row,
        mods: Mods::NONE,
        sgr_position: None,
    }
}

fn pane_hit(app: &App, id: &str) -> crate::chrome::render::PaneHit {
    app.hit_map
        .panes
        .iter()
        .find(|hit| hit.pane_id == id)
        .cloned()
        .expect("pane hit exists")
}

fn press(app: &mut App, hit: &crate::chrome::render::PaneHit, col: u16, row: u16) {
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        hit.inner_rect.x + col,
        hit.inner_rect.y + row,
    ));
}

fn drag(app: &mut App, col: u16, row: u16) {
    app.on_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), col, row));
}

fn release(app: &mut App, col: u16, row: u16) {
    app.on_mouse(mouse(MouseEventKind::Up(MouseButton::Left), col, row));
}

fn host_text(out: &Arc<Mutex<Vec<u8>>>) -> Vec<u8> {
    out.lock().unwrap().clone()
}

#[tokio::test]
async fn drag_then_release_writes_osc52_with_selected_text() {
    let mut vt = hc_vt::Vt::new(60, 10, 0);
    vt.feed(b"hello world\r\nfoo bar");
    let mut app = selection_app(vt);
    let out = capture_host(&mut app);
    let hit = pane_hit(&app, "t1");

    press(&mut app, &hit, 0, 0);
    drag(&mut app, hit.inner_rect.x + 4, hit.inner_rect.y);
    release(&mut app, hit.inner_rect.x + 4, hit.inner_rect.y);

    assert_eq!(
        host_text(&out),
        b"\x1b]52;c;aGVsbG8=\x07",
        "OSC 52 carries base64 \"hello\"",
    );
    assert!(app.selection.is_none(), "copy clears the selection");
}

#[tokio::test]
async fn mouse_tracking_pane_forwards_the_drag_and_never_selects() {
    let mut vt = hc_vt::Vt::new(60, 10, 0);
    vt.feed(b"hello world\r\n\x1b[?1000h");
    assert!(vt.mouse_tracking());
    let mut app = selection_app(vt);
    let out = capture_host(&mut app);
    let hit = pane_hit(&app, "t1");

    press(&mut app, &hit, 0, 0);
    drag(&mut app, hit.inner_rect.x + 4, hit.inner_rect.y);
    release(&mut app, hit.inner_rect.x + 4, hit.inner_rect.y);

    assert!(app.selection.is_none(), "no local selection under tracking");
    assert!(host_text(&out).is_empty(), "no OSC 52 for a reporting pane");
    assert!(
        app.pane.input_backlog().0 >= 3,
        "down/drag/up reach the program: {:?}",
        app.pane.input_backlog()
    );
}

#[tokio::test]
async fn plain_click_writes_nothing_and_clears_a_prior_selection() {
    let mut vt = hc_vt::Vt::new(60, 10, 0);
    vt.feed(b"hello world");
    let mut app = selection_app(vt);
    let out = capture_host(&mut app);
    let hit = pane_hit(&app, "t1");

    // A real selection first.
    press(&mut app, &hit, 0, 0);
    drag(&mut app, hit.inner_rect.x + 4, hit.inner_rect.y);
    release(&mut app, hit.inner_rect.x + 4, hit.inner_rect.y);
    out.lock().unwrap().clear();

    // A plain click: down + up at the same cell.
    press(&mut app, &hit, 2, 0);
    release(&mut app, hit.inner_rect.x + 2, hit.inner_rect.y);
    assert!(
        host_text(&out).is_empty(),
        "a click copies nothing: {:?}",
        String::from_utf8_lossy(&host_text(&out))
    );
    assert!(app.selection.is_none());
}

#[tokio::test]
async fn double_click_selects_word_and_drag_extends_by_words() {
    let mut vt = hc_vt::Vt::new(60, 10, 0);
    vt.feed(b"alpha beta gamma");
    let mut app = selection_app(vt);
    let out = capture_host(&mut app);
    let hit = pane_hit(&app, "t1");

    // Double-click inside "beta" (cols 6..9 inclusive).
    press(&mut app, &hit, 7, 0);
    release(&mut app, hit.inner_rect.x + 7, hit.inner_rect.y);
    press(&mut app, &hit, 7, 0);

    let selection = app.selection.as_ref().expect("word selected");
    assert_eq!(selection.ordered_cells(), ((0, 6), (0, 9)));

    // Drag into "gamma" — the end expands to the word bound, not the
    // cursor column.
    drag(&mut app, hit.inner_rect.x + 13, hit.inner_rect.y);
    let selection = app.selection.as_ref().expect("word drag extends");
    assert_eq!(selection.ordered_cells(), ((0, 6), (0, 15)));

    release(&mut app, hit.inner_rect.x + 13, hit.inner_rect.y);
    assert_eq!(
        host_text(&out),
        b"\x1b]52;c;YmV0YSBnYW1tYQ==\x07",
        "OSC 52 carries \"beta gamma\"",
    );
}

#[tokio::test]
async fn soft_wrapped_lines_extract_unwrapped() {
    let mut vt = hc_vt::Vt::new(10, 6, 0);
    vt.feed(b"abcdefghijnext");
    let mut app = selection_app(vt);
    let out = capture_host(&mut app);
    let hit = pane_hit(&app, "t1");

    // "abcdefghij" soft-wraps over rows 0..1; select through col 1 of
    // the wrap — the two screen rows join without a newline.
    press(&mut app, &hit, 0, 0);
    drag(&mut app, hit.inner_rect.x + 1, hit.inner_rect.y + 1);
    release(&mut app, hit.inner_rect.x + 1, hit.inner_rect.y + 1);

    let expected = crate::selection::osc52_sequence(b"abcdefghijne");
    assert_eq!(host_text(&out), expected.as_bytes());
}

#[tokio::test]
async fn wide_characters_extract_correctly() {
    let mut vt = hc_vt::Vt::new(60, 10, 0);
    vt.feed("ab中cd".as_bytes());
    let mut app = selection_app(vt);
    let out = capture_host(&mut app);
    let hit = pane_hit(&app, "t1");

    // a=0 b=1 中=2..3 c=4 d=5 — select the whole row.
    press(&mut app, &hit, 0, 0);
    drag(&mut app, hit.inner_rect.x + 5, hit.inner_rect.y);
    release(&mut app, hit.inner_rect.x + 5, hit.inner_rect.y);

    let expected = crate::selection::osc52_sequence("ab中cd".as_bytes());
    assert_eq!(host_text(&out), expected.as_bytes());
}

#[tokio::test]
async fn scrollback_selection_reads_history_rows() {
    let mut vt = hc_vt::Vt::new(60, 5, 64);
    for i in 1..=10 {
        vt.feed(format!("line{i:02}").as_bytes());
        if i < 10 {
            vt.feed(b"\r\n");
        }
    }
    // Scroll three rows back so history is visible, then redraw.
    vt.scroll_delta(-3);
    let mut app = selection_app(vt);
    let out = capture_host(&mut app);
    let hit = pane_hit(&app, "t1");
    let metrics = hit.scroll.expect("scrolled pane carries metrics");
    assert_eq!(metrics.offset_from_bottom, 3);

    // Viewport row 0 is screen row `max - 3` = 2 — the history row
    // showing "line03".
    press(&mut app, &hit, 0, 0);
    drag(&mut app, hit.inner_rect.x + 5, hit.inner_rect.y);
    release(&mut app, hit.inner_rect.x + 5, hit.inner_rect.y);

    let expected = crate::selection::osc52_sequence(b"line03");
    assert_eq!(
        host_text(&out),
        expected.as_bytes(),
        "viewport row 0 shows line03 while scrolled three rows back",
    );
}

#[tokio::test]
async fn new_output_during_drag_keeps_the_selected_text() {
    let mut vt = hc_vt::Vt::new(60, 5, 64);
    for i in 1..=5 {
        vt.feed(format!("line{i}").as_bytes());
        if i < 5 {
            vt.feed(b"\r\n");
        }
    }
    let mut app = selection_app(vt);
    let out = capture_host(&mut app);
    let hit = pane_hit(&app, "t1");

    press(&mut app, &hit, 0, 0);
    drag(&mut app, hit.inner_rect.x + 4, hit.inner_rect.y);
    let before = app.selection.as_ref().unwrap().ordered_cells();

    // Three new lines arrive mid-gesture — absolute rows don't move.
    app.pane.vt().unwrap().feed(b"\r\nline6\r\nline7\r\nline8");
    let after = app.selection.as_ref().unwrap().ordered_cells();
    assert_eq!(before, after, "absolute rows keep the same text");

    release(&mut app, hit.inner_rect.x + 4, hit.inner_rect.y);
    let expected = crate::selection::osc52_sequence(b"line1");
    assert_eq!(
        host_text(&out),
        expected.as_bytes(),
        "the copy still reads \"line1\" from absolute row 0",
    );
}

#[tokio::test]
async fn edge_autoscroll_scrolls_and_extends_the_selection() {
    let mut vt = hc_vt::Vt::new(60, 5, 64);
    for i in 1..=20 {
        vt.feed(format!("line{i:02}").as_bytes());
        if i < 20 {
            vt.feed(b"\r\n");
        }
    }
    vt.scroll_delta(-5);
    let mut app = selection_app(vt);
    let hit = pane_hit(&app, "t1");
    let before = app.pane.vt_ref().unwrap().scrollbar().unwrap().offset;

    press(&mut app, &hit, 0, 2);
    // Drag to (or above) the pane's top edge: the autoscroll arms and
    // each 30 ms tick walks the viewport one row further into history.
    drag(
        &mut app,
        hit.inner_rect.x,
        hit.inner_rect.y.saturating_sub(2),
    );
    assert!(
        app.selection_autoscroll.is_some(),
        "edge drag arms autoscroll"
    );

    let base = Instant::now() + Duration::from_secs(1);
    for i in 0..12 {
        app.tick_selection(base + Duration::from_millis(31 * i));
    }
    let scrolled = app.pane.vt_ref().unwrap().scrollbar().unwrap().offset;
    assert!(
        scrolled < before,
        "autoscroll moved the viewport back: {before} -> {scrolled}"
    );

    // max=15, offset_from_bottom 5 -> anchor absolute row 12; the edge
    // scroll reached the top of history so the cursor hit row 0.
    let ((start_row, _), _) = app.selection.as_ref().unwrap().ordered_cells();
    assert_eq!(start_row, 0, "cursor extended into history");
}

#[tokio::test]
async fn dragging_below_the_bottom_edge_scrolls_immediately() {
    let mut vt = hc_vt::Vt::new(60, 5, 64);
    for i in 1..=20 {
        vt.feed(format!("line{i:02}").as_bytes());
        if i < 20 {
            vt.feed(b"\r\n");
        }
    }
    vt.scroll_delta(-5);
    let mut app = selection_app(vt);
    let hit = pane_hit(&app, "t1");
    let below = hit.inner_rect.y + hit.inner_rect.height + 1;

    press(&mut app, &hit, 0, 2);
    // The pane fills the screen's last row, so the update gets the
    // out-of-pane row directly — same path `on_mouse` feeds a drag.
    app.update_selection_drag(&hit, hit.inner_rect.x, below);

    // Two rows past the edge scrolls distance*3 = 6 rows at once —
    // offset_from_bottom 5 clamps to 0 (live bottom).
    let bar = app.pane.vt_ref().unwrap().scrollbar().unwrap();
    assert_eq!(
        bar.offset,
        bar.total - bar.len,
        "immediate edge scroll snapped to the live bottom"
    );
    let autoscroll = app
        .selection_autoscroll
        .as_ref()
        .expect("bottom-edge drag arms autoscroll");
    assert!(matches!(
        autoscroll.direction,
        super::super::pane_selection::AutoscrollDirection::Down
    ));
}

#[tokio::test]
async fn copy_on_select_false_retains_highlight_then_ctrl_c_copies() {
    let mut vt = hc_vt::Vt::new(60, 10, 0);
    vt.feed(b"hello world");
    let mut app = selection_app(vt);
    app.shell.copy_on_select = false;
    let out = capture_host(&mut app);
    let hit = pane_hit(&app, "t1");

    press(&mut app, &hit, 0, 0);
    drag(&mut app, hit.inner_rect.x + 4, hit.inner_rect.y);
    release(&mut app, hit.inner_rect.x + 4, hit.inner_rect.y);

    assert!(
        host_text(&out).is_empty(),
        "release copies nothing when copy_on_select is off"
    );
    assert!(
        app.selection.as_ref().is_some_and(|s| s.is_visible()),
        "the highlight stays"
    );

    app.on_input(
        input::UiEvent::Key(term_key(Key::Char('c'), Mods::CTRL, Action::Press)),
        120,
    );
    assert_eq!(host_text(&out), b"\x1b]52;c;aGVsbG8=\x07");
    assert!(app.selection.is_none(), "Ctrl+C clears the selection");
}

#[tokio::test]
async fn highlight_renders_selected_cells_with_the_selection_style() {
    let mut vt = hc_vt::Vt::new(60, 10, 0);
    vt.feed(b"hello world\r\nplain row");
    let mut app = selection_app(vt);
    let hit = pane_hit(&app, "t1");
    let (x, y) = (hit.inner_rect.x, hit.inner_rect.y);

    press(&mut app, &hit, 0, 0);
    drag(&mut app, x + 4, y);

    let backend = draw_to(&mut app, 80, 24);
    let buf = backend.buffer();
    let selected = &buf[(x + 2, y)];
    let unselected = &buf[(x + 2, y + 1)];
    assert_ne!(
        selected.bg, unselected.bg,
        "the selected cell takes the computed selection background"
    );
}

#[tokio::test]
async fn dragging_over_the_sidebar_clamps_and_keeps_the_gesture() {
    let mut vt = hc_vt::Vt::new(60, 10, 0);
    vt.feed(b"hello world");
    let mut app = selection_app(vt);
    let out = capture_host(&mut app);
    let hit = pane_hit(&app, "t1");
    let collapsed = app.shell_state.sidebar_collapsed;

    press(&mut app, &hit, 0, 0);
    // Drag left past the pane onto the sidebar column 0 — the cursor
    // clamps to inner column 0 and the gesture still owns the release.
    drag(&mut app, 0, hit.inner_rect.y);
    release(&mut app, 0, hit.inner_rect.y);

    assert_eq!(app.shell_state.sidebar_collapsed, collapsed);
    assert_eq!(
        host_text(&out),
        b"\x1b]52;c;aA==\x07",
        "clamped single-cell selection copies \"h\"",
    );
}

//! Plain PageUp/PageDown page the focused pane's own scrollback when
//! the program's modes allow it — the reference's
//! `plain_page_keys_use_host_scrollback` plus the PageKey scroll of
//! `apply_client_terminal_input_events`. Modified, released and
//! program-eligible page keys forward as before.
use super::*;
use hc_vt::{Action, Key, Mods};

/// The pane's attach never completes; queued input stays in `pending`.
fn pending_attach(app: &mut App) {
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
}

/// A one-pane app whose pane has a VT with real scrollback.
fn page_app(vt: hc_vt::Vt) -> App {
    let mut app = app_with(&["t1"]);
    pending_attach(&mut app);
    app.reconcile_published_view();
    app.focus_pane("t1".into());
    app.pane.install_vt(vt);
    app
}

/// Five content rows plus enough history for two full pages; `setup`
/// then applies whatever program modes the case needs.
fn scrollback_vt(setup: &[u8]) -> hc_vt::Vt {
    let mut vt = hc_vt::Vt::new(40, 5, 8192);
    let mut lines = Vec::new();
    for i in 1..=20 {
        lines.extend_from_slice(format!("line-{i:02}\r\n").as_bytes());
    }
    vt.feed(&lines);
    vt.feed(setup);
    vt
}

fn scrolled_back(app: &App) -> u64 {
    let bar = app
        .pane
        .vt_ref()
        .and_then(|vt| vt.scrollbar())
        .expect("scrollbar state");
    bar.total - bar.offset - bar.len
}

fn page(key: Key, mods: Mods, action: Action) -> input::TerminalKey {
    term_key(key, mods, action)
}

#[tokio::test]
async fn plain_page_up_pages_pane_scrollback() {
    let mut app = page_app(scrollback_vt(b""));
    app.on_key(page(Key::PageUp, Mods::NONE, Action::Press), 80);
    assert_eq!(
        app.pane.input_backlog().0,
        0,
        "a paged key must not reach the program"
    );
    assert_eq!(scrolled_back(&app), 5, "one pane height");
}

#[tokio::test]
async fn plain_page_down_scrolls_back_toward_the_bottom() {
    let mut app = page_app(scrollback_vt(b""));
    for _ in 0..2 {
        app.on_key(page(Key::PageUp, Mods::NONE, Action::Press), 80);
    }
    assert_eq!(scrolled_back(&app), 10);
    app.on_key(page(Key::PageDown, Mods::NONE, Action::Press), 80);
    assert_eq!(scrolled_back(&app), 5);
    assert_eq!(app.pane.input_backlog().0, 0);
}

#[tokio::test]
async fn page_key_repeat_pages_again_and_its_release_is_swallowed() {
    let mut app = page_app(scrollback_vt(b""));
    app.on_key(page(Key::PageUp, Mods::NONE, Action::Press), 80);
    app.on_key(page(Key::PageUp, Mods::NONE, Action::Repeat), 80);
    assert_eq!(scrolled_back(&app), 10);
    app.on_key(page(Key::PageUp, Mods::NONE, Action::Release), 80);
    assert_eq!(
        app.pane.input_backlog().0,
        0,
        "release of a scrolled press must not reach the program"
    );
}

#[tokio::test]
async fn modified_page_up_still_reaches_the_program() {
    let mut app = page_app(scrollback_vt(b""));
    app.on_key(page(Key::PageUp, Mods::CTRL, Action::Press), 80);
    assert_eq!(app.pane.input_backlog().0, 1, "Ctrl+PageUp forwards");
    assert_eq!(scrolled_back(&app), 0);
    app.on_key(page(Key::PageUp, Mods::SHIFT, Action::Press), 80);
    assert_eq!(app.pane.input_backlog().0, 2, "Shift+PageUp forwards");
}

#[tokio::test]
async fn alternate_screen_page_up_reaches_the_program() {
    let mut app = page_app(scrollback_vt(b"\x1b[?1049h"));
    app.on_key(page(Key::PageUp, Mods::NONE, Action::Press), 80);
    assert_eq!(app.pane.input_backlog().0, 1);
}

#[tokio::test]
async fn mouse_reporting_page_up_reaches_the_program() {
    let mut app = page_app(scrollback_vt(b"\x1b[?1000h"));
    app.on_key(page(Key::PageUp, Mods::NONE, Action::Press), 80);
    assert_eq!(app.pane.input_backlog().0, 1);
}

#[tokio::test]
async fn bare_application_cursor_page_up_reaches_the_program() {
    let mut app = page_app(scrollback_vt(b"\x1b[?1h"));
    app.on_key(page(Key::PageUp, Mods::NONE, Action::Press), 80);
    assert_eq!(app.pane.input_backlog().0, 1);
}

#[tokio::test]
async fn application_cursor_with_bracketed_paste_still_pages() {
    let mut app = page_app(scrollback_vt(b"\x1b[?1h\x1b[?2004h"));
    app.on_key(page(Key::PageUp, Mods::NONE, Action::Press), 80);
    assert_eq!(app.pane.input_backlog().0, 0);
    assert_eq!(scrolled_back(&app), 5);
}

#[tokio::test]
async fn any_forwarded_key_returns_the_viewport_to_the_bottom() {
    let mut app = page_app(scrollback_vt(b""));
    app.on_key(page(Key::PageUp, Mods::NONE, Action::Press), 80);
    assert_eq!(scrolled_back(&app), 5);
    app.on_key(page(Key::Char('x'), Mods::NONE, Action::Press), 80);
    assert_eq!(scrolled_back(&app), 0);
    assert_eq!(app.pane.input_backlog().0, 1);
}

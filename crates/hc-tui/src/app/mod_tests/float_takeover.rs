//! Key takeover while a persistent float is focused: only popup binding
//! chords run their command (its own toggle hides the float); every other
//! key — navigation chords, Ctrl+G and Esc included — types into the
//! program, and plain page keys page the float's own scrollback.
use super::*;
use crate::app::float::FloatKey;
use crate::config::{CustomCommandAction, CustomCommandKeybind};
use crate::popup_size::PopupSize;
use hc_vt::{Action as KeyAction, Key, Mods};

fn command_labeled(label: &str) -> CustomCommandKeybind {
    CustomCommandKeybind {
        bindings: Default::default(),
        label: label.into(),
        command: "exec \"${SHELL:-sh}\"".into(),
        action: CustomCommandAction::Popup,
        description: None,
        width: Some(PopupSize::Percent(80)),
        height: Some(PopupSize::Percent(80)),
        persistent: true,
    }
}

/// The float's own terminal record, labeled like a spawned one.
fn float_term(
    address: &str,
    owner: &str,
    command: &str,
    created_ms: u64,
) -> hc_proto::terminal::Terminal {
    let mut term = crate::sidebar::fixture::term(address);
    term.created_ms = created_ms;
    term.labels.insert("hc-tui.popup".into(), "1".into());
    term.labels.insert("hc-tui.owner-tab".into(), owner.into());
    term.labels.insert("hc-tui.command".into(), command.into());
    term
}

fn pending(app: &mut App) {
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
}

/// Inventory publish carrying every live record, then reconcile.
fn publish(app: &mut App, terms: Vec<hc_proto::terminal::Terminal>) {
    let seq = app.view.inventory.seq() + 1;
    app.view.inventory_mut().replace(seq, terms);
    app.reconcile_published_view();
}

/// Spawn the focused tab's float straight into the shown state — the
/// fake `terminal.run` answer lands without a wire.
fn spawn_shown(app: &mut App, terminal: &str, created_ms: u64) -> FloatKey {
    let key = FloatKey {
        owner: app.focused_owner_key().expect("a focused tab"),
        command: "alt+f".into(),
    };
    app.run_custom_command(&command_labeled("alt+f"));
    assert!(app.popup_pending.is_some(), "the float run is armed");
    app.on_popup_spawned(terminal.into(), created_ms);
    assert!(app.popup.is_some(), "the float shows");
    key
}

/// One workspace with two tabs — the shape `previous_tab`/`next_tab`
/// cycles while a float is focused.
fn two_tabs() -> App {
    let mut app = app_with_tabs(&[("t1", "space-a", "tab-1"), ("t2", "space-a", "tab-2")]);
    pending(&mut app);
    app.reconcile_published_view();
    app
}

/// The t1/tab-1 + t2/tab-2 records matching `two_tabs`.
fn tab_terms() -> Vec<hc_proto::terminal::Terminal> {
    [("t1", "tab-1"), ("t2", "tab-2")]
        .iter()
        .enumerate()
        .map(|(i, (address, tab))| {
            let mut term = crate::sidebar::fixture::term(address);
            term.created_ms = i as u64;
            term.labels.insert("herdr.space".into(), "space-a".into());
            term.labels.insert("herdr.tab".into(), (*tab).into());
            term
        })
        .collect()
}

/// Alt+arrows navigate, `alt+f`/`alt+x` both run the persistent shell
/// popup — the chords the takeover resolves.
fn float_live() -> crate::config::LiveKeybindConfig {
    let config: crate::config::Config = toml::from_str(
        "[keys]\n\
         previous_tab = \"alt+left\"\n\
         next_tab = \"alt+right\"\n\n\
         [[keys.command]]\n\
         key = \"alt+f\"\n\
         type = \"popup\"\n\
         command = \"exec \\\"${SHELL:-sh}\\\"\"\n\n\
         [[keys.command]]\n\
         key = \"alt+x\"\n\
         type = \"popup\"\n\
         command = \"exec \\\"${SHELL:-sh}\\\"\"\n",
    )
    .unwrap();
    config.live_keybinds()
}

fn press_key(app: &mut App, key: input::TerminalKey) {
    app.on_input(input::UiEvent::Key(key), 120);
}

/// A shown float over `two_tabs`, published like a spawned one.
fn shown_float() -> (App, FloatKey) {
    let mut app = two_tabs();
    app.live = float_live();
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    let key = spawn_shown(&mut app, "t9", 9);
    let mut terms = tab_terms();
    terms.push(float_term("t9", &key.owner, "alt+f", 9));
    publish(&mut app, terms);
    // The setup spawn hit the dead wire; only the takeover is under test.
    app.notice = None;
    (app, key)
}

fn float_backlog(app: &App) -> usize {
    app.popup
        .as_ref()
        .expect("the float keeps focus")
        .pane
        .input_backlog()
        .0
}

#[tokio::test]
async fn a_nav_chord_types_into_the_float() {
    let (mut app, _) = shown_float();
    press_key(
        &mut app,
        term_key(Key::ArrowRight, Mods::ALT, KeyAction::Press),
    );
    assert_eq!(
        app.popup.as_ref().map(|popup| popup.terminal.as_str()),
        Some("t9"),
        "the float keeps focus: no tab switch"
    );
    assert_eq!(float_backlog(&app), 1, "the chord reached the program");
    assert_eq!(
        app.pane.input_backlog().0,
        0,
        "nothing reached the pane behind"
    );
}

#[tokio::test]
async fn ctrl_g_types_into_the_float_instead_of_locking() {
    let (mut app, _) = shown_float();
    press_key(
        &mut app,
        term_key(Key::Char('g'), Mods::CTRL, KeyAction::Press),
    );
    assert!(!app.keyboard_locked, "the float takes Ctrl+G");
    assert_eq!(float_backlog(&app), 1);
}

#[tokio::test]
async fn a_locked_keyboard_still_types_into_the_float() {
    let (mut app, _) = shown_float();
    app.keyboard_locked = true;
    press_key(
        &mut app,
        term_key(Key::Char('x'), Mods::NONE, KeyAction::Press),
    );
    assert_eq!(float_backlog(&app), 1, "the shown float owns the keys");
    assert_eq!(app.pane.input_backlog().0, 0, "not the pane behind it");
    // Its own chord still hides it, so Ctrl+G can unlock again.
    press_key(
        &mut app,
        term_key(Key::Char('f'), Mods::ALT, KeyAction::Press),
    );
    assert!(
        app.popup.is_none(),
        "Alt+F hides the float even when locked"
    );
}

/// Five content rows plus two pages of history, on the normal screen.
fn scrollback_vt() -> hc_vt::Vt {
    let mut vt = hc_vt::Vt::new(40, 5, 8192);
    let mut lines = Vec::new();
    for i in 1..=20 {
        lines.extend_from_slice(format!("line-{i:02}\r\n").as_bytes());
    }
    vt.feed(&lines);
    vt
}

fn float_scrolled_back(app: &App) -> u64 {
    let bar = app
        .popup
        .as_ref()
        .and_then(|popup| popup.pane.vt_ref())
        .and_then(|vt| vt.scrollbar())
        .expect("scrollbar state");
    bar.total - bar.offset - bar.len
}

#[tokio::test]
async fn page_keys_page_the_floats_own_scrollback() {
    let (mut app, _) = shown_float();
    app.host_reports_key_releases = true;
    app.popup.as_mut().unwrap().pane.install_vt(scrollback_vt());
    press_key(
        &mut app,
        term_key(Key::PageUp, Mods::NONE, KeyAction::Press),
    );
    assert_eq!(float_scrolled_back(&app), 5, "one float height");
    assert_eq!(
        float_backlog(&app),
        0,
        "a paged key never reaches the program"
    );
    press_key(
        &mut app,
        term_key(Key::PageUp, Mods::NONE, KeyAction::Release),
    );
    assert_eq!(float_backlog(&app), 0, "nor does its release");
    press_key(
        &mut app,
        term_key(Key::PageUp, Mods::NONE, KeyAction::Press),
    );
    press_key(
        &mut app,
        term_key(Key::PageDown, Mods::NONE, KeyAction::Press),
    );
    assert_eq!(float_scrolled_back(&app), 5, "PageDown pages back");
    assert_eq!(app.pane.input_backlog().0, 0, "the pane behind never moves");

    press_key(
        &mut app,
        term_key(Key::Char('x'), Mods::NONE, KeyAction::Press),
    );
    assert_eq!(float_backlog(&app), 1, "other keys still type");
    assert_eq!(
        float_scrolled_back(&app),
        0,
        "and jump the view back to the bottom"
    );
}

#[tokio::test]
async fn page_keys_reach_a_float_program_that_pages_itself() {
    let (mut app, _) = shown_float();
    let mut vt = scrollback_vt();
    vt.feed(b"\x1b[?1049h");
    app.popup.as_mut().unwrap().pane.install_vt(vt);
    press_key(
        &mut app,
        term_key(Key::PageUp, Mods::NONE, KeyAction::Press),
    );
    assert_eq!(float_backlog(&app), 1, "the alternate screen gets its keys");
}

#[tokio::test]
async fn an_intercepted_chords_release_never_reaches_the_float() {
    let mut app = two_tabs();
    app.live = float_live();
    app.host_reports_key_releases = true;
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    let key_a = spawn_shown(&mut app, "t9", 9);
    // A second persistent float replaces the first: t9 parks hidden.
    app.run_custom_command(&command_labeled("alt+x"));
    app.on_popup_spawned("t8".into(), 8);
    assert_eq!(app.popup.as_ref().unwrap().terminal, "t8");
    let mut terms = tab_terms();
    terms.push(float_term("t9", &key_a.owner, "alt+f", 9));
    terms.push(float_term("t8", &key_a.owner, "alt+x", 8));
    publish(&mut app, terms);

    // t9's own chord brings it back — the press navigated the popup
    // slot, so its release must not type into the promoted program.
    press_key(
        &mut app,
        term_key(Key::Char('f'), Mods::ALT, KeyAction::Press),
    );
    assert_eq!(app.popup.as_ref().unwrap().terminal, "t9");
    assert!(app.popup_key_suppress.contains(&Key::Char('f')));
    press_key(
        &mut app,
        term_key(Key::Char('f'), Mods::ALT, KeyAction::Release),
    );
    assert_eq!(
        app.popup.as_ref().unwrap().pane.input_backlog().0,
        0,
        "no release reaches a program the press never typed into"
    );
    assert!(app.popup_key_suppress.is_empty());
}

#[tokio::test]
async fn keys_without_a_takeover_binding_still_reach_the_float() {
    let mut app = two_tabs();
    app.live = float_live();
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    let key = spawn_shown(&mut app, "t9", 9);
    let mut terms = tab_terms();
    terms.push(float_term("t9", &key.owner, "alt+f", 9));
    publish(&mut app, terms);

    // `next_agent` is unbound — its chord types through like any key.
    press_key(
        &mut app,
        term_key(Key::ArrowDown, Mods::ALT, KeyAction::Press),
    );
    press_key(
        &mut app,
        term_key(Key::Char('x'), Mods::NONE, KeyAction::Press),
    );
    press_key(
        &mut app,
        term_key(Key::Escape, Mods::NONE, KeyAction::Press),
    );
    let popup = app.popup.as_ref().expect("the float keeps focus");
    assert_eq!(popup.pane.input_backlog().0, 3);
}

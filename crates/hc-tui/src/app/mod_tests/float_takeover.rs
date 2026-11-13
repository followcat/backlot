//! Key takeover while a persistent float is focused: navigation chords
//! still navigate (the float parks or stays with its tab), popup binding
//! chords run their command, and everything else types into the program.
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
    app.view.inventory.replace(seq, terms);
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

#[tokio::test]
async fn a_nav_chord_switches_tabs_and_parks_the_float() {
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

    press_key(
        &mut app,
        term_key(Key::ArrowRight, Mods::ALT, KeyAction::Press),
    );
    assert!(app.popup.is_none(), "the float parks on a tab switch");
    assert!(
        app.size_leases.contains_key("t9"),
        "its program lives on the size lease"
    );
    assert!(
        app.floats.get(&key).is_some_and(|entry| entry.restore),
        "leaving visible means returning re-shows it"
    );
    assert_eq!(app.pending_actions, 0);
    assert!(app.notice.is_none(), "parking issues no terminal.kill");

    press_key(
        &mut app,
        term_key(Key::ArrowLeft, Mods::ALT, KeyAction::Press),
    );
    assert_eq!(
        app.popup.as_ref().map(|popup| popup.terminal.as_str()),
        Some("t9"),
        "switching back restores the float"
    );
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

//! Alt+L (`[keys] focus_lot`) tests: show/focus the lot on its last
//! selectable row, stay resting (never compose), jump again when
//! already focused, scroll long lists, fall through under `--plain`,
//! rebind/unbind via `[keys]`, yield to floats/modals/compose, and
//! surface discoverability hints in the header, help, and launcher.

use super::*;
use crate::app::input::UiEvent;
use crate::config::{CustomCommandAction, CustomCommandKeybind};
use crate::hollywood::LayerMode;
use crate::panel::{PanelMode, Target};
use crate::popup_size::PopupSize;
use crate::test_support::TestDir;

fn key_mods(app: &mut App, k: hc_vt::Key, mods: hc_vt::Mods) {
    app.on_input(
        UiEvent::Key(term_key(k, mods, hc_vt::Action::Press)),
        app.area.0,
    );
}

fn alt_l(app: &mut App) {
    key_mods(app, hc_vt::Key::Char('l'), hc_vt::Mods::ALT);
}

fn alt_o(app: &mut App) {
    key_mods(app, hc_vt::Key::Char('o'), hc_vt::Mods::ALT);
}

pub(super) fn app_with_studio_and_talk() -> App {
    let mut app = app_with_panel_doc();
    let mut doc = summary_json();
    doc["studios"][0]["exchange"] = serde_json::json!([
        { "seq": 10, "who": "user", "text": "check build" },
        { "seq": 11, "who": "producer", "text": "build green" }
    ]);
    doc["hollywood"] = serde_json::json!({
        "state": "live",
        "runner": "fake",
        "model": "m",
        "effort": "high"
    });
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.set_hollywood(
        Ok(Some(backlot_stage::config::HollywoodCfg {
            runner: "fake".into(),
            model: "m".into(),
            effort: Some("high".into()),
            key: None,
            default_cast: None,
        })),
        std::path::Path::new("config.toml"),
    );
    app.hollywood.mode = LayerMode::Collapsed;
    app
}

#[tokio::test]
async fn alt_l_from_pane_with_lot_hidden_enters_studio_on_priority_row() {
    let mut app = app_with_studio_and_talk();
    app.area = (120, 24);
    app.panel.mode = PanelMode::Closed;
    app.panel.focused = false;
    app.panel.selected = None;

    let before = app.pane.input_backlog().0;
    alt_l(&mut app);

    assert_eq!(app.panel.mode, PanelMode::Expanded);
    assert!(app.panel.focused);
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "actor-bob".into())),
        "Needs-you blocked actor-bob is selected on studio entry"
    );
    assert!(!app.panel.editing, "entering studio must not start compose");
    assert_eq!(app.pane.input_backlog().0, before, "alt+l was consumed");

    // 2nd Alt+L inside studio starts compose on `›`; Esc rests then unfocuses straight to Terminal (1 studio).
    alt_l(&mut app);
    assert!(app.panel_editing());
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.panel.editing);
    assert!(app.panel.focused);
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.panel.focused);
    assert_eq!(app.panel.mode, PanelMode::Expanded);

    // Narrow desktop (80x24): unfocused dock auto-hides; Alt+L opens overlay inside studio, Esc closes.
    app.area = (80, 24);
    assert_eq!(app.panel_mode_at(80), PanelMode::Closed);
    alt_l(&mut app);
    assert_eq!(app.panel_mode_at(80), PanelMode::Expanded);
    assert!(app.panel.focused);
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "actor-bob".into()))
    );
    assert!(!app.panel.editing);
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.panel.mode, PanelMode::Closed);
}

#[tokio::test]
async fn alt_l_from_collapsed_strip_expands_and_enters_studio() {
    let mut app = app_with_studio_and_talk();
    app.area = (120, 24);
    app.panel.mode = PanelMode::Collapsed;
    app.panel.focused = false;
    draw_to(&mut app, 120, 24);

    alt_l(&mut app);

    assert_eq!(app.panel.mode, PanelMode::Expanded);
    assert!(app.panel.focused);
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "actor-bob".into()))
    );
    assert!(!app.panel.editing);
}

#[tokio::test]
async fn alt_l_with_lot_open_unfocused_enters_current_studio_priority_row() {
    let mut app = app_with_studio_and_talk();
    app.area = (120, 24);
    app.panel.mode = PanelMode::Expanded;
    app.panel.focused = false;
    app.panel.selected = Some(Target::Person("acme-shop".into(), "producer".into()));

    alt_l(&mut app);

    assert!(app.panel.focused);
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "actor-bob".into()))
    );
    assert!(!app.panel.editing);
}

#[tokio::test]
async fn alt_l_inside_the_lot_enters_compose_on_second_press_and_never_closes() {
    let mut app = app_with_studio_and_talk();
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "producer".into()));
    app.sync_focus();

    alt_l(&mut app);
    assert_eq!(app.panel.mode, PanelMode::Expanded);
    assert!(app.panel.focused);
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    assert!(app.panel.editing);

    // Pressing Alt+L again while composing keeps it open and composing.
    alt_l(&mut app);
    assert_eq!(app.panel.mode, PanelMode::Expanded);
    assert!(app.panel.focused);
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    assert!(app.panel.editing);
}

#[tokio::test]
async fn alt_l_scrolls_long_lot_list_so_landing_and_compose_rows_are_visible() {
    let mut app = app_with_studio_and_talk();
    let mut doc = overflow_doc();
    doc.value["hollywood"] = serde_json::json!({
        "state": "live",
        "runner": "fake",
        "model": "m",
        "effort": "high"
    });
    app.view.documents_mut().insert(doc_key(), doc.into());
    app.reconcile_published_view();
    app.hollywood.mode = LayerMode::Collapsed;
    app.area = (120, 20);
    app.panel.mode = PanelMode::Closed;
    app.panel.focused = false;
    app.panel.scroll = 0;

    alt_l(&mut app);
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "actor-bob".into()))
    );
    assert!(!app.panel.editing);

    // 2nd Alt+L jumps to Input and scrolls it into viewport.
    alt_l(&mut app);
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    assert!(app.panel.editing);
    let text = screen_lines(&draw_to(&mut app, 120, 20)).join("\n");
    assert!(
        text.contains("tell producer"),
        "compose row must be visible after 2nd Alt+L:\n{text}"
    );
    assert!(
        app.panel_hits
            .rows
            .iter()
            .any(|(_, target)| *target == Target::Input("acme-shop".into())),
        "Input row must be within the rendered viewport"
    );
}

#[tokio::test]
async fn alt_l_without_hollywood_layer_reaches_the_program() {
    let mut app = app_with_studio_and_talk();
    app.area = (120, 24);
    app.set_plain(true);
    let before = app.pane.input_backlog().0;

    alt_l(&mut app);

    assert!(
        app.pane.input_backlog().0 > before,
        "alt+l must reach the program under --plain"
    );
    assert_eq!(app.panel.mode, PanelMode::Closed);
    assert!(!app.panel.focused);
}

#[tokio::test]
async fn focus_lot_config_rebinds_and_unbinds() {
    let mut app = app_with_studio_and_talk();
    app.area = (120, 24);
    let dir = TestDir::new("focus-lot-cfg");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");

    // Rebind to alt+o: alt+l reaches program, alt+o focuses the lot's last row.
    std::fs::write(&path, "[keys]\nfocus_lot = \"alt+o\"\n").unwrap();
    let (config, notice) = crate::panel::load(&path);
    assert!(notice.is_none());
    app.set_panel_config(config, notice, &path);
    app.panel.mode = PanelMode::Closed;
    app.panel.focused = false;

    let before = app.pane.input_backlog().0;
    alt_l(&mut app);
    assert!(app.pane.input_backlog().0 > before, "old alt+l unbound");
    assert_eq!(app.panel.mode, PanelMode::Closed);

    alt_o(&mut app);
    assert_eq!(app.panel.mode, PanelMode::Expanded);
    assert!(app.panel.focused);
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "actor-bob".into()))
    );

    // Unbind with empty string: neither alt+l nor alt+o triggers.
    std::fs::write(&path, "[keys]\nfocus_lot = \"\"\n").unwrap();
    let (config, notice) = crate::panel::load(&path);
    assert!(notice.is_none());
    app.set_panel_config(config, notice, &path);
    app.panel.mode = PanelMode::Closed;
    app.panel.focused = false;

    let before = app.pane.input_backlog().0;
    alt_l(&mut app);
    assert!(app.pane.input_backlog().0 > before);
    assert_eq!(app.panel.mode, PanelMode::Closed);
}

#[tokio::test]
async fn alt_l_does_not_fire_inside_float_modal_or_compose() {
    // 1. Persistent float: no change to the lot.
    let mut app = app_with_studio_and_talk();
    app.area = (120, 24);
    app.panel.mode = PanelMode::Closed;
    app.panel.focused = false;
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    app.run_custom_command(&CustomCommandKeybind {
        bindings: Default::default(),
        label: "alt+f".into(),
        command: "exec \"${SHELL:-sh}\"".into(),
        action: CustomCommandAction::Popup,
        description: None,
        width: Some(PopupSize::Percent(80)),
        height: Some(PopupSize::Percent(80)),
        persistent: true,
    });
    app.on_popup_spawned("t50".into(), 50);
    assert!(app.popup.is_some());
    alt_l(&mut app);
    assert_eq!(app.panel.mode, PanelMode::Closed);
    assert!(!app.panel.focused);

    // 1b. Ephemeral popup: no change to the lot.
    app.popup = None;
    app.popup_pending = None;
    app.run_custom_command(&CustomCommandKeybind {
        bindings: Default::default(),
        label: "alt+h".into(),
        command: "echo hi".into(),
        action: CustomCommandAction::Popup,
        description: None,
        width: Some(PopupSize::Percent(80)),
        height: Some(PopupSize::Percent(80)),
        persistent: false,
    });
    app.on_popup_spawned("t51".into(), 51);
    assert!(app.popup.is_some());
    alt_l(&mut app);
    assert_eq!(app.panel.mode, PanelMode::Closed);
    assert!(!app.panel.focused);

    // 2. Help modal, Task Sheet modal, CardView modal, and studio confirm: no change.
    let mut app = app_with_studio_and_talk();
    app.area = (120, 24);
    app.panel.mode = PanelMode::Closed;
    app.panel.focused = false;
    app.help = true;
    alt_l(&mut app);
    assert_eq!(app.panel.mode, PanelMode::Closed);
    assert!(!app.panel.focused);
    app.help = false;

    app.sheet_open(crate::studio::StudioKey::local("acme-shop"), None);
    assert!(app.sheet.is_some());
    alt_l(&mut app);
    assert_eq!(app.panel.mode, PanelMode::Closed);
    assert!(!app.panel.focused);
    app.sheet = None;

    app.hollywood.view = Some(crate::hollywood::CardView {
        card: None,
        title: "test".into(),
        lines: vec!["line".into()],
        scroll: 0,
        ..crate::hollywood::CardView::default()
    });
    alt_l(&mut app);
    assert_eq!(app.panel.mode, PanelMode::Closed);
    assert!(!app.panel.focused);
    app.hollywood.view = None;

    app.studio_dialog = Some(crate::studio_dialog::StudioDialog::Confirm(
        crate::studio_dialog::ConfirmDialog {
            title: "Confirm?".into(),
            body: "Proceed".into(),
            loading: false,
            error: None,
            for_: crate::studio_dialog::ConfirmFor::StageStop { machine: None },
        },
    ));
    alt_l(&mut app);
    assert_eq!(app.panel.mode, PanelMode::Closed);
    assert!(!app.panel.focused);
    app.studio_dialog = None;

    // 3. Running compose in Dock, finish confirm in Dock, or Call Sheet compose: keep keys.
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "producer".into()));
    app.panel.finish_confirm = Some("acme-shop".into());
    alt_l(&mut app);
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "producer".into()))
    );
    app.panel.finish_confirm = None;

    key(&mut app, hc_vt::Key::Char('i'));
    assert!(app.panel_editing());
    alt_l(&mut app);
    assert!(app.panel_editing(), "dock compose keeps its keys");

    app.panel.editing = false;
    app.panel.focused = false;
    app.panel.selected = Some(Target::Person("acme-shop".into(), "producer".into()));
    key_mods(&mut app, hc_vt::Key::Char('y'), hc_vt::Mods::ALT);
    assert!(app.hollywood.expanded() && app.hollywood.editing);
    alt_l(&mut app);
    assert!(app.hollywood.expanded() && app.hollywood.editing);
    assert!(!app.panel.focused);
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "producer".into()))
    );
}

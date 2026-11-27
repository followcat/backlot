//! Group 1 fail-first tests: FocusState / WorkspaceRegion / LotLevel,
//! global region entry (Alt+L, Ctrl+B t, Ctrl+B a, Alt+Y), removal of
//! Tree & CallSheet global swallowing, Ctrl+G gating, modal focus
//! restoration, and click-anywhere + single-click Lot person decision.

use super::*;
use crate::app::input::{MouseButton, MouseEventKind, MouseInput, UiEvent};
use crate::focus::{CallSheetZone, LotLevel, ModalLayer, WorkspaceRegion};
use crate::hollywood::LayerMode;
use crate::panel::{PanelMode, Target};
use crate::studio::StudioKey;
use hc_proto::watch::{DocKey, Document};

fn key(app: &mut App, k: hc_vt::Key) {
    key_mods(app, k, hc_vt::Mods::NONE);
}

fn key_mods(app: &mut App, k: hc_vt::Key, mods: hc_vt::Mods) {
    app.on_input(
        UiEvent::Key(term_key(k, mods, hc_vt::Action::Press)),
        app.area.0,
    );
}

fn press_prefix(app: &mut App, ch: char) {
    key_mods(app, hc_vt::Key::Char('b'), hc_vt::Mods::CTRL);
    key(app, hc_vt::Key::Char(ch));
}

fn click(app: &mut App, col: u16, row: u16) {
    app.on_mouse(MouseInput {
        kind: MouseEventKind::Down(MouseButton::Left),
        col,
        row,
        mods: hc_vt::Mods::NONE,
        sgr_position: None,
    });
}

fn two_studio_focus_app(has_ask_in_acme: bool) -> App {
    let mut app = app_with(&["t1", "t2", "t3", "t4", "t7"]);
    app.set_plain(false);
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    let terminals: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .cloned()
        .chain(std::iter::once(stage))
        .collect();
    app.view.inventory_mut().replace(2, terminals);
    let acme_producer = if has_ask_in_acme {
        serde_json::json!({
            "role": "producer", "name": "producer", "terminal": "t1",
            "state": "live", "work": "waiting",
            "asks": "Ship OAuth v2 or keep v1?", "asks_seq": 48, "since_ms": 1_000
        })
    } else {
        serde_json::json!({
            "role": "producer", "name": "producer", "terminal": "t1",
            "state": "live", "work": "working"
        })
    };
    let doc = serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 100,
        "hollywood": { "state": "live", "runner": "fake", "model": "m", "effort": "high" },
        "studios": [
            {
                "id": "acme-shop", "title": "Acme shop", "state": "live",
                "requirement_version": 2, "mode": "standard",
                "open_reviews": 0, "waiting": usize::from(has_ask_in_acme),
                "roles": [
                    acme_producer,
                    { "role": "director", "name": "director", "terminal": "t2", "state": "live", "work": "working" },
                    { "role": "actor-alice", "name": "alice", "terminal": "t4", "runner": "agent-a", "model": "m", "state": "live", "work": "working", "task": "t1" }
                ],
                "exchange": [
                    { "seq": 10, "who": "user", "text": "prioritize v2" },
                    { "seq": 11, "who": "producer", "text": "on it" }
                ]
            },
            {
                "id": "beta-api", "title": "Beta API", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "roles": [
                    { "role": "producer", "name": "producer", "terminal": "t7", "state": "live", "work": "waiting" }
                ]
            }
        ]
    });
    app.view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
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
    app.focused = Some("t4".into());
    app.studio.current = Some(StudioKey::local("acme-shop"));
    app.sync_target();
    app.reconcile_published_view();
    app.hollywood.mode = LayerMode::Collapsed;
    app.panel.mode = PanelMode::Expanded;
    app.panel.focused = false;
    app.area = (140, 36);
    app
}

#[tokio::test]
async fn alt_l_enters_current_studio_and_second_press_starts_compose() {
    // Active studio is acme-shop with open ask on producer -> first Alt+L enters
    // Lot(InsideStudio(acme-shop)) on the open ask (not beta-api, not SetupHint, not Input).
    let mut app = two_studio_focus_app(true);
    app.panel.mode = PanelMode::Closed;
    key_mods(&mut app, hc_vt::Key::Char('l'), hc_vt::Mods::ALT);

    assert_eq!(app.panel.mode, PanelMode::Expanded);
    assert!(app.panel.focused);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::InsideStudio(StudioKey::local("acme-shop")))
    );
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "producer".into())),
        "Ask owner in Needs You should be selected, got {:?}",
        app.panel.selected
    );
    assert!(!app.panel.editing);

    // Without Needs You, Alt+L lands on active terminal's member (actor-alice = t4).
    let mut app2 = two_studio_focus_app(false);
    app2.focused = Some("t4".into());
    app2.studio.current = Some(StudioKey::local("acme-shop"));
    key_mods(&mut app2, hc_vt::Key::Char('l'), hc_vt::Mods::ALT);
    assert_eq!(
        app2.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::InsideStudio(StudioKey::local("acme-shop")))
    );
    assert_eq!(
        app2.panel.selected,
        Some(Target::Person("acme-shop".into(), "actor-alice".into())),
        "Active pane member must be selected on entry, got {:?}",
        app2.panel.selected
    );

    // 2nd Alt+L while inside acme-shop jumps into Compose on acme-shop!
    key_mods(&mut app2, hc_vt::Key::Char('l'), hc_vt::Mods::ALT);
    assert!(app2.panel.editing, "2nd Alt+L inside studio enters Compose");
    assert!(matches!(
        app2.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::Compose { ref studio, .. }) if studio.id == "acme-shop"
    ));

    // Pressing Alt+L in Compose is a no-op (keeps editing).
    key_mods(&mut app2, hc_vt::Key::Char('l'), hc_vt::Mods::ALT);
    assert!(app2.panel.editing);

    // Stepping back with Esc: Compose -> InsideStudio -> Studios (>=2 studios) -> Terminal.
    key(&mut app2, hc_vt::Key::Escape);
    assert!(!app2.panel.editing);
    assert_eq!(
        app2.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::InsideStudio(StudioKey::local("acme-shop")))
    );
    key(&mut app2, hc_vt::Key::Escape);
    assert_eq!(
        app2.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::Studios)
    );
    key(&mut app2, hc_vt::Key::Escape);
    assert_eq!(app2.focus.workspace, WorkspaceRegion::Terminal);
}

#[tokio::test]
async fn esc_inside_single_studio_lot_skips_straight_to_terminal() {
    let mut app = two_studio_focus_app(false);
    // Single studio doc: Esc inside InsideStudio skips Studios and goes straight to Terminal.
    let single = serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 100,
        "studios": [{
            "id": "acme-shop", "title": "Acme shop", "state": "live",
            "requirement_version": 1, "mode": "standard", "open_reviews": 0, "waiting": 0,
            "roles": [{ "role": "producer", "name": "producer", "terminal": "t1", "state": "live" }]
        }]
    });
    app.view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        Document {
            value: single,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    key_mods(&mut app, hc_vt::Key::Char('l'), hc_vt::Mods::ALT);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::InsideStudio(StudioKey::local("acme-shop")))
    );
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Terminal,
        "1 studio skips Studios on Esc"
    );
}

#[tokio::test]
async fn tree_cast_and_call_sheet_do_not_swallow_global_region_chords() {
    let mut app = two_studio_focus_app(false);
    draw_to(&mut app, 140, 36);

    // 1. From Tree: Alt+L enters Lot(InsideStudio), Alt+Y opens Call Sheet.
    press_prefix(&mut app, 't');
    assert_eq!(app.focus.workspace, WorkspaceRegion::Tree);
    key_mods(&mut app, hc_vt::Key::Char('l'), hc_vt::Mods::ALT);
    assert!(!app.tree_focused());
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::InsideStudio(StudioKey::local("acme-shop")))
    );

    press_prefix(&mut app, 't');
    assert_eq!(app.focus.workspace, WorkspaceRegion::Tree);
    key_mods(&mut app, hc_vt::Key::Char('y'), hc_vt::Mods::ALT);
    assert!(!app.tree_focused());
    assert!(app.hollywood.expanded());
    assert!(matches!(app.focus.workspace, WorkspaceRegion::CallSheet(_)));

    // 2. From CallSheet (even resting on a frame): Alt+L collapses Call Sheet and enters that frame's studio!
    app.hollywood.editing = false;
    app.hollywood.selected = Some(crate::hollywood::Target::Studio(StudioKey::local(
        "beta-api",
    )));
    app.focus.workspace = WorkspaceRegion::CallSheet(CallSheetZone::Studios);
    key_mods(&mut app, hc_vt::Key::Char('l'), hc_vt::Mods::ALT);
    assert!(!app.hollywood.expanded());
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::InsideStudio(StudioKey::local("beta-api")))
    );

    // 3. Ctrl+B a opens Cast (and expands collapsed sidebar) & Alt+Y / Alt+L leave Cast cleanly.
    key(&mut app, hc_vt::Key::Escape);
    key(&mut app, hc_vt::Key::Escape);
    app.shell_state.sidebar_collapsed = true;
    press_prefix(&mut app, 'a');
    assert!(!app.shell_state.sidebar_collapsed);
    assert_eq!(app.focus.workspace, WorkspaceRegion::Cast);

    // Tab switches Cast <-> Tree.
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(app.focus.workspace, WorkspaceRegion::Tree);
    key(&mut app, hc_vt::Key::BackTab);
    assert_eq!(app.focus.workspace, WorkspaceRegion::Cast);

    // Alt+L leaves Cast into Cast's studio (acme-shop) in Lot.
    key_mods(&mut app, hc_vt::Key::Char('l'), hc_vt::Mods::ALT);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::InsideStudio(StudioKey::local("acme-shop")))
    );
}

#[tokio::test]
async fn ctrl_g_locked_outside_terminal_and_under_all_modals() {
    let mut app = two_studio_focus_app(false);

    // In Lot: Ctrl+G must NOT lock.
    key_mods(&mut app, hc_vt::Key::Char('l'), hc_vt::Mods::ALT);
    key_mods(&mut app, hc_vt::Key::Char('g'), hc_vt::Mods::CTRL);
    assert!(!app.keyboard_locked, "Ctrl+G blocked in Lot");
    key(&mut app, hc_vt::Key::Escape);
    key(&mut app, hc_vt::Key::Escape);

    // In Tree: Ctrl+G must NOT lock.
    press_prefix(&mut app, 't');
    key_mods(&mut app, hc_vt::Key::Char('g'), hc_vt::Mods::CTRL);
    assert!(!app.keyboard_locked, "Ctrl+G blocked in Tree");
    key(&mut app, hc_vt::Key::Escape);

    // In Cast: Ctrl+G must NOT lock.
    press_prefix(&mut app, 'a');
    key_mods(&mut app, hc_vt::Key::Char('g'), hc_vt::Mods::CTRL);
    assert!(!app.keyboard_locked, "Ctrl+G blocked in Cast");
    key(&mut app, hc_vt::Key::Escape);

    // Under TaskSheet & UpdateCard & HwSwitch & Reader: Ctrl+G must NOT lock.
    app.sheet_open(StudioKey::local("acme-shop"), None);
    key_mods(&mut app, hc_vt::Key::Char('g'), hc_vt::Mods::CTRL);
    assert!(!app.keyboard_locked, "Ctrl+G blocked under TaskSheet");
    app.sheet = None;
    app.sync_focus();

    app.update_latest = Some("9.9.9".into());
    app.open_update();
    assert!(app.update_modal.is_some());
    key_mods(&mut app, hc_vt::Key::Char('g'), hc_vt::Mods::CTRL);
    assert!(!app.keyboard_locked, "Ctrl+G blocked under UpdateCard");
    app.update_modal = None;
    app.sync_focus();

    app.hollywood.view = Some(crate::hollywood::CardView::default());
    key_mods(&mut app, hc_vt::Key::Char('g'), hc_vt::Mods::CTRL);
    assert!(!app.keyboard_locked, "Ctrl+G blocked under Reader");
    app.hollywood.view = None;
    app.sync_focus();

    app.studio_dialog = Some(crate::studio_dialog::StudioDialog::Confirm(
        crate::studio_dialog::ConfirmDialog {
            title: "Confirm?".into(),
            body: "Proceed".into(),
            loading: false,
            error: None,
            for_: crate::studio_dialog::ConfirmFor::StageStop { machine: None },
        },
    ));
    key_mods(&mut app, hc_vt::Key::Char('g'), hc_vt::Mods::CTRL);
    assert!(!app.keyboard_locked, "Ctrl+G blocked under StudioDialog");
    app.studio_dialog = None;
    app.sync_focus();

    app.hw_switch_open(false);
    key_mods(&mut app, hc_vt::Key::Char('g'), hc_vt::Mods::CTRL);
    assert!(!app.keyboard_locked, "Ctrl+G blocked under HwSwitch");
    app.hw_switch = None;
    app.sync_focus();

    // Back in Terminal with empty modals: Ctrl+G DOES toggle keyboard_locked.
    assert_eq!(app.focus.workspace, WorkspaceRegion::Terminal);
    assert!(app.focus.modals.is_empty());
    key_mods(&mut app, hc_vt::Key::Char('g'), hc_vt::Mods::CTRL);
    assert!(app.keyboard_locked, "Ctrl+G toggles lock in Terminal");
    key_mods(&mut app, hc_vt::Key::Char('g'), hc_vt::Mods::CTRL);
    assert!(!app.keyboard_locked);
}

#[tokio::test]
async fn modal_pop_restores_exact_underlying_region() {
    let mut app = two_studio_focus_app(false);
    draw_to(&mut app, 140, 36);

    // Open menu from Lot(InsideStudio) -> Esc restores Lot(InsideStudio).
    key_mods(&mut app, hc_vt::Key::Char('l'), hc_vt::Mods::ALT);
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(app.menu.is_some());
    assert!(app.focus.modals.contains(&ModalLayer::ContextMenu));
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.menu.is_none());
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::InsideStudio(StudioKey::local("acme-shop")))
    );

    // Open TaskSheet via 'v' -> Esc restores Lot(InsideStudio).
    key(&mut app, hc_vt::Key::Char('v'));
    assert!(app.sheet.is_some());
    assert!(app.focus.modals.contains(&ModalLayer::TaskSheet));
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.sheet.is_none());
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::InsideStudio(StudioKey::local("acme-shop")))
    );
}

#[tokio::test]
async fn single_click_on_unfocused_lot_person_switches_pane_and_keeps_lot_focus() {
    let mut app = two_studio_focus_app(false);
    assert_eq!(app.focused.as_deref(), Some("t4"));
    assert_eq!(app.focus.workspace, WorkspaceRegion::Terminal);
    draw_to(&mut app, 140, 36);

    // Click producer row (t1) in Lot while Terminal has focus (Decision #2).
    let rect = app
        .panel_hits
        .rows
        .iter()
        .find_map(|(r, t)| {
            (*t == Target::Person("acme-shop".into(), "producer".into())).then_some(*r)
        })
        .expect("producer row hit in Lot");
    click(&mut app, rect.x + 2, rect.y);

    assert_eq!(
        app.focused.as_deref(),
        Some("t1"),
        "main pane switched to t1"
    );
    assert!(app.panel.focused, "focus stays in Lot");
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::InsideStudio(StudioKey::local("acme-shop")))
    );
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "producer".into()))
    );
    // Arrow down still walks rows inside Lot!
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_ne!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "producer".into()))
    );
}

#[tokio::test]
async fn focus_tree_and_focus_cast_config_rebinds_and_click_anywhere_focuses_regions() {
    let mut app = two_studio_focus_app(false);
    let dir = crate::test_support::TestDir::new("focus-tree-cast-cfg");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");

    // Rebind focus_cast = "alt+a" and focus_tree = "alt+t"
    std::fs::write(
        &path,
        "[keys]\nfocus_cast = \"alt+a\"\nfocus_tree = \"alt+t\"\n",
    )
    .unwrap();
    let (config, notice) = crate::panel::load(&path);
    assert!(notice.is_none(), "{notice:?}");
    app.set_panel_config(config, notice, &path);

    key_mods(&mut app, hc_vt::Key::Char('a'), hc_vt::Mods::ALT);
    assert_eq!(app.focus.workspace, WorkspaceRegion::Cast);

    key_mods(&mut app, hc_vt::Key::Char('t'), hc_vt::Mods::ALT);
    assert_eq!(app.focus.workspace, WorkspaceRegion::Tree);

    // Draw and test click-anywhere-to-focus: clicking Cast switches focus to Cast,
    // clicking Tree switches to Tree, clicking main pane switches to Terminal.
    draw_to(&mut app, 140, 36);
    let cast_hit = app
        .hit_map
        .agents
        .iter()
        .find(|(_, id)| id == "t2")
        .map(|(r, _)| *r)
        .expect("director cast hit");
    click(&mut app, cast_hit.x + 1, cast_hit.y);
    assert_eq!(app.focused.as_deref(), Some("t2"));
    assert_eq!(app.focus.workspace, WorkspaceRegion::Cast);

    let pane_hit = app.hit_map.panes[0].inner_rect;
    click(&mut app, pane_hit.x + 1, pane_hit.y + 1);
    assert_eq!(app.focus.workspace, WorkspaceRegion::Terminal);
}

#[tokio::test]
async fn terminal_passthrough_preserves_tab_esc_alt_arrows_and_ctrl_g() {
    let mut app = two_studio_focus_app(false);
    assert_eq!(app.focus.workspace, WorkspaceRegion::Terminal);
    assert!(app.focus.modals.is_empty());

    // Tab, Shift+Tab, Esc, Alt+Left, and Alt+Right must reach the terminal verbatim.
    for (k, mods) in [
        (hc_vt::Key::Tab, hc_vt::Mods::NONE),
        (hc_vt::Key::BackTab, hc_vt::Mods::SHIFT),
        (hc_vt::Key::Escape, hc_vt::Mods::NONE),
        (hc_vt::Key::ArrowLeft, hc_vt::Mods::ALT),
        (hc_vt::Key::ArrowRight, hc_vt::Mods::ALT),
    ] {
        let before = app.pane.input_backlog().0;
        key_mods(&mut app, k, mods);
        assert!(
            app.pane.input_backlog().0 > before,
            "key ({k:?}, {mods:?}) must reach focused terminal in studio mode"
        );
    }

    // Outside a studio workspace (or under --plain), Alt+Up/Down also reach the terminal.
    let mut plain = two_studio_focus_app(false);
    plain.set_plain(true);
    for k in [hc_vt::Key::ArrowUp, hc_vt::Key::ArrowDown] {
        let before = plain.pane.input_backlog().0;
        key_mods(&mut plain, k, hc_vt::Mods::ALT);
        assert!(
            plain.pane.input_backlog().0 > before,
            "key ({k:?}, ALT) must reach terminal outside studio workspace / --plain"
        );
    }

    // Ctrl+G locks keyboard so even Alt+L and Alt+PageDown forward to the terminal until Ctrl+G unlocks.
    key_mods(&mut app, hc_vt::Key::Char('g'), hc_vt::Mods::CTRL);
    assert!(app.keyboard_locked);
    for (k, mods) in [
        (hc_vt::Key::Char('l'), hc_vt::Mods::ALT),
        (hc_vt::Key::PageDown, hc_vt::Mods::ALT),
        (hc_vt::Key::ArrowUp, hc_vt::Mods::ALT),
    ] {
        let before = app.pane.input_backlog().0;
        key_mods(&mut app, k, mods);
        assert!(
            app.pane.input_backlog().0 > before,
            "locked terminal must forward ({k:?}, {mods:?})"
        );
    }
    key_mods(&mut app, hc_vt::Key::Char('g'), hc_vt::Mods::CTRL);
    assert!(!app.keyboard_locked);

    // Custom Herdr prefix bindings on `t` / `a` must not be swallowed in Terminal focus.
    app.live.keybinds.toggle_sidebar = crate::config::ActionKeybinds {
        bindings: vec![
            crate::config::ResolvedBinding {
                trigger: crate::config::BindingTrigger::Prefix((
                    crossterm::event::KeyCode::Char('t'),
                    crossterm::event::KeyModifiers::NONE,
                )),
                label: "prefix+t".into(),
            },
            crate::config::ResolvedBinding {
                trigger: crate::config::BindingTrigger::Prefix((
                    crossterm::event::KeyCode::Char('a'),
                    crossterm::event::KeyModifiers::NONE,
                )),
                label: "prefix+a".into(),
            },
        ],
    };
    let collapsed_before = app.shell_state.sidebar_collapsed;
    press_prefix(&mut app, 't');
    assert_eq!(app.focus.workspace, WorkspaceRegion::Terminal);
    assert_eq!(
        app.shell_state.sidebar_collapsed, !collapsed_before,
        "custom Herdr prefix+t kept routing to Herdr ToggleSidebar instead of Tree"
    );
    press_prefix(&mut app, 'a');
    assert_eq!(app.focus.workspace, WorkspaceRegion::Terminal);
    assert_eq!(
        app.shell_state.sidebar_collapsed, collapsed_before,
        "custom Herdr prefix+a kept routing to Herdr ToggleSidebar instead of Cast"
    );
}

//! U1/R3 `Alt+arrow` hardening: after a studio or role switch the
//! keys must belong to the newly focused pane — never to a dock, a
//! `›` line, the Call Sheet or a CardView that stayed up over it.
//! Studio shells are first-class members of `current`/`remembered`,
//! entry from a non-studio workspace lands on the current studio
//! first, and remote studios cycle the same way. Neutral fixtures
//! only (`acme-shop`, `night-owl`, `beta-tools`, `ghost`, `host-b`).

use super::studio::{alt, doc_key, two_studio_app};
use super::studio_machines::two_machine_studio_app;
use super::*;
use crate::app::input::UiEvent;
use crate::hollywood::LayerMode;
use crate::panel::{PanelMode, Target};
use crate::studio::StudioKey;
use hc_proto::terminal::Terminal;
use hc_proto::watch::Document;

/// A live studio shell: `backlot.studio` + `backlot.shell` labels in
/// the studio's own space, the shape `studio_shell` creates.
fn shell(address: &str, studio: &str, n: u32) -> Terminal {
    let mut t = crate::sidebar::fixture::term(address);
    t.labels
        .insert(crate::shells::STUDIO_LABEL.into(), studio.into());
    t.labels
        .insert(crate::shells::SHELL_LABEL.into(), n.to_string());
    t.labels.insert("herdr.space".into(), studio.into());
    t.labels.insert("herdr.tab".into(), format!("shell {n}"));
    t.name = Some(crate::shells::shell_name(studio, n));
    t
}

/// A plain tab in a user workspace — no `backlot.*` labels at all.
fn plain_tab(address: &str, space: &str, tab: &str) -> Terminal {
    let mut t = crate::sidebar::fixture::term(address);
    t.labels.insert("herdr.space".into(), space.into());
    t.labels.insert("herdr.tab".into(), tab.into());
    t
}

/// The `bl-hollywood` terminal — a Hollywood header, not a studio's.
fn hollywood(address: &str) -> Terminal {
    let mut t = plain_tab(address, "hollywood", "main");
    t.name = Some(backlot_model::hollywood::HOLLYWOOD_TERMINAL.into());
    t
}

/// Push terminals into the inventory — the same shape a stage or a
/// `prefix+t` publish takes.
fn add_terminals(app: &mut App, extra: Vec<Terminal>) {
    let terms: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .cloned()
        .chain(extra)
        .collect();
    app.view.inventory_mut().replace(2, terms);
    app.reconcile_published_view();
}

/// The two-studios-plus-ghost document, plus two `acme-shop` shells,
/// a `ghost` shell, a two-tab user workspace and the Hollywood
/// terminal. Focus starts on acme's producer `t1`.
pub(super) fn alt_app() -> App {
    let mut app = two_studio_app();
    add_terminals(
        &mut app,
        vec![
            shell("t30", "acme-shop", 1),
            shell("t31", "acme-shop", 2),
            shell("t41", "ghost", 1),
            plain_tab("t20", "notes", "tab-1"),
            plain_tab("t21", "notes", "tab-2"),
            hollywood("t40"),
        ],
    );
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    app
}

/// A plain character press — what "typing afterwards" looks like.
fn type_char(app: &mut App, c: char) {
    app.on_input(
        UiEvent::Key(term_key(
            hc_vt::Key::Char(c),
            hc_vt::Mods::NONE,
            hc_vt::Action::Press,
        )),
        app.area.0,
    );
}

fn typed_reaches_pane(app: &mut App) -> bool {
    let before = app.pane.input_backlog().0;
    type_char(app, 'a');
    app.pane.input_backlog().0 > before
}

// ---------- keyboard ownership after a switch ----------

#[tokio::test]
async fn a_studio_step_from_the_focused_dock_keeps_the_dock_and_swaps_the_pane() {
    let mut app = alt_app();
    app.area = (120, 24);
    app.panel.mode = PanelMode::Expanded;
    app.panel.focused = true;
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("night-owl".into()));
    // The step landed night-owl's remembered member in the main pane —
    // its producer here — and `StudioFocus` follows it to a role.
    assert_eq!(app.focused.as_deref(), Some("t7"));
    assert_eq!(
        app.studio.focus,
        Some(crate::studio::StudioFocus::Role(
            StudioKey::local("night-owl"),
            "t7".into()
        ))
    );
    // The dock still owns the keys, with its cursor on the studio the
    // step landed on — the person can keep stepping.
    assert!(app.panel_focused(), "the dock kept the keys");
    assert_eq!(
        app.panel.selected.as_ref().and_then(|t| t.studio()),
        Some(&StudioKey::local("night-owl"))
    );
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("ghost".into()));
    assert_eq!(
        app.focused.as_deref(),
        Some("t41"),
        "ghost's live shell lands"
    );
}

#[tokio::test]
async fn a_studio_switch_leaves_a_composing_docks_input_line() {
    let mut app = alt_app();
    app.area = (120, 24);
    app.panel.mode = PanelMode::Expanded;
    app.panel.focused = true;
    app.panel.selected = Some(Target::Input(StudioKey::local("acme-shop")));
    app.panel.editing = true;
    assert!(app.panel_editing());
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("night-owl".into()));
    assert!(
        !app.panel_editing(),
        "the old studio's › line lost the keys"
    );
    assert!(typed_reaches_pane(&mut app), "typing reaches the pane");
}

#[tokio::test]
async fn a_studio_switch_collapses_the_call_sheet() {
    let mut app = alt_app();
    app.hollywood.mode = LayerMode::Expanded;
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("night-owl".into()));
    assert_eq!(app.focused.as_deref(), Some("t7"));
    // Part 1 §1.2.6: while resting in CallSheet, Alt+PageDown keeps CallSheet
    // open and syncs its frame selection to the switched studio behind it.
    assert!(app.hollywood.expanded(), "CallSheet remains open in resting mode");
    assert_eq!(
        app.hollywood.selected,
        Some(crate::hollywood::Target::Studio("night-owl".into()))
    );
}

#[tokio::test]
async fn a_studio_switch_leaves_the_call_sheets_input() {
    let mut app = alt_app();
    app.hollywood.mode = LayerMode::Expanded;
    app.hollywood.editing = true;
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("night-owl".into()));
    assert_eq!(app.focused.as_deref(), Some("t7"));
    assert!(!app.hollywood.editing, "the tell line lost the keys");
    assert!(typed_reaches_pane(&mut app));
}

#[tokio::test]
async fn a_studio_switch_closes_an_open_cardview() {
    let mut app = alt_app();
    app.hollywood.view = Some(crate::hollywood::CardView {
        card: None,
        title: "hollywood".into(),
        lines: vec!["a report".into()],
        scroll: 0,
        ..crate::hollywood::CardView::default()
    });
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("night-owl".into()));
    assert_eq!(app.focused.as_deref(), Some("t7"));
    assert!(
        app.hollywood.view.is_none(),
        "no read-only card floats over the pane"
    );
    assert!(typed_reaches_pane(&mut app));
}

#[tokio::test]
async fn non_text_modal_overlays_dismiss_on_studio_nav() {
    // Non-text-entry overlays (`help`, `menu`, `sheet`, `navigator`, `peek`)
    // dismiss on `Alt+arrows` so navigation works from any non-input surface.
    let mut app = alt_app();
    app.help = true;
    alt(&mut app, hc_vt::Key::PageDown);
    assert!(!app.help, "help dismissed");
    assert_eq!(app.studio.current, Some("night-owl".into()));
}

// ---------- studio shells ----------

#[tokio::test]
async fn focusing_a_shell_moves_the_current_studio() {
    let mut app = alt_app();
    // Leave acme for night-owl, then focus acme's shell 1 by hand —
    // the same way a click or a tab switch lands there.
    app.focus_pane("t7".into());
    assert_eq!(app.studio.current, Some("night-owl".into()));
    app.focus_pane("t30".into());
    assert_eq!(
        app.studio.current,
        Some("acme-shop".into()),
        "the shell's studio became current"
    );
}

#[tokio::test]
async fn the_role_keys_step_from_a_shell_through_its_studios_cast() {
    let mut app = alt_app();
    app.focus_pane("t30".into());
    // acme's cycle: producer t1, alice t4, shell 1 (t30), shell 2 (t31).
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.focused.as_deref(), Some("t31"), "shell 1 → shell 2");
    assert_eq!(app.studio.current, Some("acme-shop".into()));
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.focused.as_deref(), Some("t1"), "wrapped to the cast");
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.focused.as_deref(), Some("t4"));
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.focused.as_deref(), Some("t30"), "roles → shells");
}

#[tokio::test]
async fn returning_to_a_studio_lands_on_its_remembered_shell() {
    let mut app = alt_app();
    app.focus_pane("t30".into());
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("night-owl".into()));
    alt(&mut app, hc_vt::Key::PageUp);
    assert_eq!(app.studio.current, Some("acme-shop".into()));
    assert_eq!(
        app.focused.as_deref(),
        Some("t30"),
        "the shell the studio was left on lands"
    );
}

#[tokio::test]
async fn a_studio_with_only_live_shells_focuses_one() {
    let mut app = alt_app();
    // ghost's only role is closed — but its shell is live.
    app.focus_pane("t7".into());
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("ghost".into()));
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(
        app.focused.as_deref(),
        Some("t41"),
        "the live shell landed, not a no-live-terminal notice"
    );
    assert!(app.notice.is_none());
}

// ---------- entry from outside a studio workspace ----------

#[tokio::test]
async fn alt_pagedown_from_the_stage_workspace_enters_the_current_studio() {
    let mut app = alt_app();
    app.focus_pane("t9".into()); // the stage terminal, `backlot` space
    assert!(!app.studio.lot.studio_workspace);
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("acme-shop".into()));
    assert_eq!(
        app.studio.focus,
        Some(crate::studio::StudioFocus::Role(
            StudioKey::local("acme-shop"),
            "t1".into()
        ))
    );
    assert_eq!(
        app.focused.as_deref(),
        Some("t1"),
        "the first press enters and lands the studio's member"
    );
    // The next press steps studios.
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("night-owl".into()));
    assert_eq!(app.focused.as_deref(), Some("t7"));
}

#[tokio::test]
async fn alt_pagedown_from_the_hollywood_terminal_enters_the_current_studio() {
    let mut app = alt_app();
    app.focus_pane("t40".into());
    assert!(!app.studio.lot.studio_workspace);
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("acme-shop".into()));
    assert_eq!(
        app.studio.focus,
        Some(crate::studio::StudioFocus::Role(
            StudioKey::local("acme-shop"),
            "t1".into()
        ))
    );
    assert_eq!(app.focused.as_deref(), Some("t1"));
}

#[tokio::test]
async fn alt_pagedown_from_a_user_workspace_enters_the_current_studio() {
    let mut app = alt_app();
    app.focus_pane("t20".into());
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("acme-shop".into()));
    assert_eq!(
        app.studio.focus,
        Some(crate::studio::StudioFocus::Role(
            StudioKey::local("acme-shop"),
            "t1".into()
        ))
    );
    assert_eq!(app.focused.as_deref(), Some("t1"));
}

#[tokio::test]
async fn outside_a_studio_workspace_role_keys_reach_herdr_while_studio_keys_enter() {
    // Herdr's usual layout: Alt+Left/Right for tabs, Alt+Up/Down for agents.
    // Outside a studio workspace, Alt+Down reaches Herdr's binding while
    // Alt+PgDn enters the current studio.
    let config: crate::config::Config = toml::from_str(
        "[keys]\n\
         previous_tab = 'alt+left'\n\
         next_tab = 'alt+right'\n\
         previous_agent = 'alt+up'\n\
         next_agent = 'alt+down'\n",
    )
    .unwrap();
    let mut app = alt_app();
    app.live = config.live_keybinds();
    app.focus_pane("t50".into());
    assert!(!app.studio.lot.studio_workspace);
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(
        app.focused.as_deref(),
        Some("t51"),
        "Alt+Down fell through to herdr's next_agent outside studio workspace"
    );
    assert_eq!(app.studio.focus, None);
    alt(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(
        app.focused.as_deref(),
        Some("t50"),
        "Alt+Up fell through to herdr's previous_agent"
    );

    // From a plain tab workspace, Alt+PgDn enters the current studio
    // and lands its member.
    app.focus_pane("t20".into());
    assert!(!app.studio.lot.studio_workspace);
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(
        app.studio.focus,
        Some(crate::studio::StudioFocus::Role(
            StudioKey::local("acme-shop"),
            "t1".into()
        )),
        "Alt+PgDn entered the current studio from plain workspace"
    );
    assert_eq!(app.focused.as_deref(), Some("t1"));
}

// ---------- remote studios ----------

/// host-b's `beta-tools` plus a live studio shell there.
fn remote_app() -> App {
    let mut app = two_machine_studio_app();
    add_terminals(&mut app, vec![shell("host-b/t20", "beta-tools", 1)]);
    app
}

#[tokio::test]
async fn alt_pagedown_reaches_the_remote_studio_in_aggregate_order() {
    let mut app = remote_app();
    // Local `acme-shop` first, then host-b's `beta-tools`, then wrap.
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(
        app.studio.current,
        Some(StudioKey {
            machine: Some("host-b".into()),
            id: "beta-tools".into()
        })
    );
    assert_eq!(
        app.focused.as_deref(),
        Some("host-b/t1"),
        "the remote studio's producer lands"
    );
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.studio.current, Some("acme-shop".into()), "wrapped");
    assert_eq!(app.focused.as_deref(), Some("t1"));
}

#[tokio::test]
async fn the_role_keys_cycle_a_remote_studios_roles_then_shells() {
    let mut app = remote_app();
    app.focus_pane("host-b/t1".into());
    assert_eq!(
        app.studio.current,
        Some(StudioKey {
            machine: Some("host-b".into()),
            id: "beta-tools".into()
        })
    );
    // beta-tools: producer host-b/t1, actor-b host-b/t10, shell host-b/t20.
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.focused.as_deref(), Some("host-b/t10"));
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.focused.as_deref(), Some("host-b/t20"), "then its shell");
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.focused.as_deref(), Some("host-b/t1"), "wrapped");
}

#[tokio::test]
async fn focusing_a_remote_shell_moves_the_current_studio() {
    let mut app = remote_app();
    assert_eq!(app.studio.current, Some("acme-shop".into()));
    app.focus_pane("host-b/t20".into());
    assert_eq!(
        app.studio.current,
        Some(StudioKey {
            machine: Some("host-b".into()),
            id: "beta-tools".into()
        }),
        "the remote shell's studio became current"
    );
}

// ---------- rendered header ----------

#[tokio::test]
async fn the_header_follows_the_switch_wide_and_mobile() {
    let mut app = alt_app();
    app.area = (120, 24);
    alt(&mut app, hc_vt::Key::PageDown);
    assert_eq!(app.focused.as_deref(), Some("t7"));
    let backend = draw_to(&mut app, 120, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("night-owl › Ⓟ"), "wide header: {text}");
    // The mobile header keeps the same `id › part` shape.
    let backend = draw_to(&mut app, 40, 24);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("night-owl"), "mobile header: {text}");
}

// ---------- ghost row: the fixture's own doc_key reuse ----------

#[tokio::test]
async fn a_republished_document_keeps_the_shell_membership() {
    let mut app = alt_app();
    app.focus_pane("t30".into());
    assert_eq!(app.studio.current, Some("acme-shop".into()));
    // A publish that changes nothing else — the shell's membership
    // and the remembered landing survive.
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: serde_json::json!({
                "schema": "backlot/1", "updated_ms": 9,
                "studios": [{
                    "id": "acme-shop", "title": "Acme shop", "state": "live",
                    "requirement_version": 1, "mode": "standard",
                    "open_reviews": 0, "waiting": 0,
                    "roles": [
                        { "role": "producer", "name": "producer", "terminal": "t1",
                          "state": "live", "work": "working" },
                        { "role": "actor-alice", "name": "alice", "terminal": "t4",
                          "state": "live", "work": "working", "task": "task-1" }
                    ]
                }]
            }),
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    assert_eq!(app.studio.current, Some("acme-shop".into()));
    alt(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(
        app.focused.as_deref(),
        Some("t4"),
        "shell 1 steps back to the last live role"
    );
}

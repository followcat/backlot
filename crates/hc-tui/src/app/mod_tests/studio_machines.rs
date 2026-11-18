//! U6a machines and the studio cast: with several endpoints the
//! expanded sidebar keeps the current studio's cast under the machine
//! tree — the section the single-endpoint sidebar already drew — while
//! `--plain` keeps herdr's endpoint sidebar exactly. Neutral fixtures
//! only (`host-b`, `acme-shop`, `alice`).

use super::endpoint_nav::{app_multi, remote};
use super::*;
use crate::app::input::{MouseButton, MouseEventKind, MouseInput};
use crate::sidebar::fixture::{agent, machine, term};
use hc_proto::watch::{DocKey, Document};
use ratatui::style::Modifier;

/// The local `backlot` document: `acme-shop` with its producer,
/// director, writer and the actors alice and bob.
fn local_summary_json() -> serde_json::Value {
    serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "studios": [{
            "id": "acme-shop", "title": "Acme shop", "state": "live",
            "requirement_version": 1, "mode": "standard",
            "open_reviews": 0, "waiting": 0,
            "roles": [
                { "role": "producer", "name": "producer", "terminal": "t1",
                  "state": "live", "work": "working",
                  "runner": "agent-a", "model": "claude-m" },
                { "role": "director", "name": "director", "terminal": "t2",
                  "state": "live", "work": "waiting", "runner": "agent-a" },
                { "role": "writer", "name": "writer", "terminal": "t3",
                  "state": "live" },
                { "role": "actor-alice", "name": "alice", "terminal": "t4",
                  "state": "live", "work": "working", "task": "task-1",
                  "runner": "agent-b", "model": "m" },
                { "role": "actor-bob", "name": "bob", "terminal": "t5",
                  "state": "live", "work": "blocked", "task": "task-2" }
            ]
        }]
    })
}

/// host-b's document: `beta-tools`. Its bare role terminals parse into
/// `host-b/<terminal>` — studios span the machines, so the cast header
/// exercises its `id · machine` label.
fn remote_summary_json() -> serde_json::Value {
    serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 2,
        "studios": [{
            "id": "beta-tools", "title": "Beta tools", "state": "live",
            "requirement_version": 1, "mode": "standard",
            "open_reviews": 0, "waiting": 0,
            "roles": [
                { "role": "producer", "name": "producer", "terminal": "t1",
                  "state": "live", "work": "waiting" },
                { "role": "actor-b", "name": "actor-b", "terminal": "t10",
                  "state": "live", "work": "working", "task": "task-1",
                  "runner": "agent-a", "model": "m" }
            ]
        }]
    })
}

fn doc_key(machine: Option<&str>) -> DocKey {
    DocKey {
        machine: machine.map(str::to_owned),
        name: "backlot".into(),
    }
}

fn spaced(address: &str, space: &str) -> hc_proto::terminal::Terminal {
    let mut t = term(address);
    t.labels.insert("herdr.space".into(), space.into());
    t.labels.insert("herdr.tab".into(), "main".into());
    t
}

/// Two endpoints and an active studio: `Local` carries acme-shop's
/// people in the `acme-shop` space and the stage plus one stray agent
/// in `backlot`; `host-b` carries beta-tools' people. `t1` — the
/// producer — is focused, so the local `acme-shop` is current.
pub(super) fn two_machine_studio_app() -> App {
    let mut stage = spaced("t9", "backlot");
    stage.name = Some("backlot-stage".into());
    let mut stray = agent("t50", "idle");
    stray.labels.insert("herdr.space".into(), "backlot".into());
    stray.labels.insert("herdr.tab".into(), "main".into());
    let mut remote_stage = spaced("host-b/t9", "backlot");
    remote_stage.name = Some("backlot-stage".into());
    let mut app = app_multi(
        vec![
            spaced("t1", "acme-shop"),
            spaced("t2", "acme-shop"),
            spaced("t3", "acme-shop"),
            spaced("t4", "acme-shop"),
            spaced("t5", "acme-shop"),
            stage,
            stray,
            spaced("host-b/t1", "beta-tools"),
            spaced("host-b/t2", "beta-tools"),
            spaced("host-b/t10", "beta-tools"),
            remote_stage,
        ],
        vec![machine("host-b", "connected")],
    );
    app.set_plain(false);
    app.view.documents_mut().insert(
        doc_key(None),
        Document {
            value: local_summary_json(),
            ..Document::default()
        }
        .into(),
    );
    app.view.documents_mut().insert(
        doc_key(Some("host-b")),
        Document {
            value: remote_summary_json(),
            ..Document::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    // U7: the bar would share the top rows these tests read; hide it
    // here — its own tests own it.
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    app
}

fn mouse(kind: MouseEventKind, col: u16, row: u16) -> MouseInput {
    MouseInput {
        kind,
        col,
        row,
        mods: hc_vt::Mods::NONE,
        sgr_position: None,
    }
}

/// The sidebar's 26 columns, one string per row, right-trimmed.
fn sidebar_text(b: &TestBackend) -> String {
    let (h, buf) = (b.size().unwrap().height, b.buffer());
    (0..h)
        .map(|y| {
            (0..26)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn the_endpoint_sidebar_keeps_the_cast_under_the_machine_tree() {
    let mut app = two_machine_studio_app();
    app.area = (120, 24);
    let backend = draw_to(&mut app, 120, 24);
    let sidebar = sidebar_text(&backend);
    // The machine tree still shows both endpoints above the cast.
    assert!(sidebar.contains("machines"), "{sidebar}");
    assert!(sidebar.contains("Local"), "{sidebar}");
    assert!(sidebar.contains("host-b"), "{sidebar}");
    // The lower section is the current studio's cast — the
    // single-endpoint sidebar's section, herdr's agents header gone.
    // A8.3: the machine tag is never dropped whole — it truncates
    // inside `id@machine` instead.
    assert!(sidebar.contains("cast · acme-…@Local"), "{sidebar}");
    assert!(sidebar.contains("producer"), "{sidebar}");
    assert!(sidebar.contains("director"), "{sidebar}");
    assert!(sidebar.contains("alice task-1"), "{sidebar}");
    assert!(sidebar.contains("others 1"), "{sidebar}");
    assert!(!sidebar.contains("agents"), "{sidebar}");
    // The draggable section split is gone, as in the one-machine path.
    assert!(app.hit_map.sidebar_section_divider.is_empty());
    assert!(app.hit_map.endpoint_agents.is_empty());
}

#[tokio::test]
async fn the_cast_hit_rows_focus_and_the_others_row_expands() {
    let mut app = two_machine_studio_app();
    app.area = (120, 24);
    draw_to(&mut app, 120, 24);
    // Clicking alice's cast row focuses her terminal, as the
    // single-endpoint cast's rows do.
    let alice = app
        .hit_map
        .agents
        .iter()
        .find(|(_, t)| t == "t4")
        .map(|(rect, _)| *rect)
        .expect("alice's cast row hit");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        alice.x,
        alice.y,
    ));
    assert_eq!(app.focused.as_deref(), Some("t4"));
    assert_eq!(app.studio.current, Some("acme-shop".into()));
    // The `others` row expands in place.
    let others = app.hit_map.cast_others;
    assert!(!others.is_empty(), "the others row hit");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        others.x,
        others.y,
    ));
    assert!(app.studio.others_expanded);
}

#[tokio::test]
async fn the_new_dot_row_opens_the_dialog_with_the_active_machine() {
    // U6b: `new · <machine>` is the endpoint sidebar's new-studio
    // entry — its click opens the native dialog with the active
    // endpoint's machine preselected.
    let mut app = two_machine_studio_app();
    app.projection.active_endpoint_id = remote("host-b");
    app.area = (120, 24);
    draw_to(&mut app, 120, 24);
    let rect = app.hit_map.new_workspace;
    assert!(!rect.is_empty(), "the `new ·` row has a hit");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 1,
        rect.y,
    ));
    let d = app.new_studio.as_ref().expect("the dialog opened");
    assert_eq!(d.machine_sel, 1, "host-b preselected");
    assert_eq!(d.pane, crate::new_studio::Step1Pane::Repos);
}

#[tokio::test]
async fn the_classic_new_row_keeps_the_workspace_flow() {
    // One endpoint — the classic ` new` footer — still routes through
    // `new_workspace`: studioless lands on the opening page rather
    // than on the dialog directly.
    let mut app = app_multi(vec![term("t1")], vec![]);
    app.set_plain(false);
    app.opening.settled = true;
    app.area = (120, 24);
    draw_to(&mut app, 120, 24);
    let rect = app.hit_map.new_workspace;
    assert!(!rect.is_empty(), "the classic ` new` row has a hit");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 1,
        rect.y,
    ));
    assert!(app.new_studio.is_none(), "no direct dialog");
    assert!(app.opening_visible(), "the opening page came up");
}

#[tokio::test]
async fn plain_mode_keeps_herdrs_endpoint_sidebar_unchanged() {
    // `--plain` never builds the cast: the two endpoints draw herdr's
    // expanded sidebar — the draggable split, `agents` with its sort
    // label and the per-machine agent rows — cell for cell.
    let mut app = two_machine_studio_app();
    app.set_plain(true);
    app.area = (120, 24);
    let backend = draw_to(&mut app, 120, 24);
    let sidebar = sidebar_text(&backend);
    let expected = expected_plain();
    assert_eq!(sidebar, expected, "\n--- sidebar ---\n{sidebar}\n---");
    // The headers and the split divider keep herdr's styling.
    let palette = &app.shell.palette;
    let buf = backend.buffer();
    let cell = |x: u16, y: u16| buf[(x, y)].clone();
    assert_eq!(cell(0, 0).symbol(), " ");
    assert_eq!(cell(1, 0).symbol(), "m");
    assert_eq!(cell(1, 0).fg, palette.overlay0);
    assert!(cell(1, 0).modifier.contains(Modifier::BOLD));
    assert_eq!(cell(0, 12).symbol(), "─");
    assert_eq!(cell(0, 12).fg, palette.surface_dim);
    assert_eq!(cell(1, 13).symbol(), "a");
    assert_eq!(cell(1, 13).fg, palette.overlay0);
    assert!(cell(1, 13).modifier.contains(Modifier::BOLD));
    assert!(!app.hit_map.sidebar_section_divider.is_empty());
    assert!(!app.hit_map.endpoint_agents.is_empty());
    assert!(app.hit_map.agents.is_empty());
    assert!(app.hit_map.cast_others.is_empty());
    // A second draw of the same fixture is byte-identical, styles
    // included.
    let again = draw_to(&mut app, 120, 24);
    assert_eq!(backend.buffer(), again.buffer());
}

// The `--plain` sidebar cell for cell — herdr's expanded endpoint
// sidebar: the draggable split's divider, `agents` + `grouped`, the
// stray agent row and the collapse toggle. The header right-aligns
// the running crate's version.
fn expected_plain() -> String {
    let version = format!("v{}", crate::app::update_modal::RUNNING);
    format!(" {:<8}{:>15} │\n{EXPECTED_PLAIN_TAIL}", "machines", version)
}

const EXPECTED_PLAIN_TAIL: &str = concat!(
    "                         │\n",
    " ▾ Local                 │\n",
    "   · acme-shop           │\n",
    "   ○ backlot             │\n",
    " ▾ host-b               ●│\n",
    "   · beta-tools          │\n",
    "   · backlot             │\n",
    "                         │\n",
    "                         │\n",
    "                         │\n",
    " new · Local         menu│\n",
    "─────────────────────────│\n",
    " agents          priority│\n",
    "                         │\n",
    " ○ Local · backlot · main│\n",
    "                         │\n",
    "                         │\n",
    "                         │\n",
    "                         │\n",
    "                         │\n",
    "                         │\n",
    "                         │\n",
    "                        «│",
);

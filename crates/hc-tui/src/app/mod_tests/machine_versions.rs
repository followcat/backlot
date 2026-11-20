//! U6c per-machine backlot versions: the sidebar's version lines,
//! the machine menu's install/upgrade/stage items, the env-probe
//! refusal, the install popup's pinned line and the stage restart
//! chain. Neutral fixtures only (`host-b`, `acme-shop`, `beta-tools`).

use super::endpoint_nav::remote;
use super::studio_machines::two_machine_studio_app;
use super::*;
use crate::app::input::{MouseButton, MouseEventKind, MouseInput, UiEvent};
use crate::machine_versions::Backlot;
use crate::sidebar::fixture::machine;
use crate::studio_menu::{MenuKind, StudioAct};
use hc_proto::watch::DocKey;
use tokio::sync::mpsc;

/// The running binary's version — Local's own line is built from it.
fn running() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// The running version one step lower — its patch when that can drop,
/// otherwise its minor, otherwise its major. A machine at this version
/// is genuinely behind `running()`, so the upgrade paths hold for any
/// release the workspace ships, with no version spelled out here.
fn older() -> String {
    let current = env!("CARGO_PKG_VERSION");
    let (head, patch) = current.rsplit_once('.').expect("x.y.z");
    let patch: u32 = patch.parse().expect("numeric patch");
    if patch > 0 {
        return format!("{head}.{}", patch - 1);
    }
    let (major, minor) = head.split_once('.').expect("x.y");
    let minor: u32 = minor.parse().expect("numeric minor");
    if minor > 0 {
        return format!("{major}.{}", minor - 1);
    }
    let major: u32 = major.parse().expect("numeric major");
    assert!(major > 0, "no version below 0.0.0");
    format!("{}.0.0", major - 1)
}

/// `two_machine_studio_app` whose `host-b` document publishes
/// `backlot_version` — the field an upgraded stage writes.
fn versioned_app(remote_version: &str) -> App {
    let mut app = two_machine_studio_app();
    let key = DocKey {
        machine: Some("host-b".into()),
        name: "backlot".into(),
    };
    let doc = app
        .view
        .documents_mut()
        .get_mut(&key)
        .expect("host-b document");
    std::sync::Arc::make_mut(doc).value["backlot_version"] =
        serde_json::Value::String(remote_version.into());
    app
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

/// The whole screen's joined text — a notice card's body sits in it.
fn screen_text(b: &TestBackend) -> String {
    screen_lines(b).join("\n")
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

fn key(app: &mut App, k: hc_vt::Key, mods: hc_vt::Mods) {
    app.on_input(
        UiEvent::Key(term_key(k, mods, hc_vt::Action::Press)),
        app.area.0,
    );
}

/// The menu's labels, in draw order.
fn labels_of(menu: &crate::studio_menu::StudioMenu) -> Vec<String> {
    menu.items().into_iter().map(|it| it.label).collect()
}

/// A remote runner whose conns land nowhere read — `machine_install`
/// only needs it to be `Some`; the env probe's reply rides
/// `on_mv_reply` in tests.
fn dead_remote_runner() -> crate::panel::RemoteRunner {
    let (tx, _rx) = mpsc::channel(1);
    let (rpc, _sub) = crate::testcore::openers(tx);
    crate::panel::RemoteRunner::new(rpc)
}

#[tokio::test]
async fn each_machine_shows_its_backlot_version() {
    let mut app = versioned_app(&older());
    app.area = (120, 36);
    let backend = draw_to(&mut app, 120, 36);
    let sidebar = sidebar_text(&backend);
    // Local's line reads the running binary's version; host-b's doc
    // answered its own — plus `hc <v>` from `machine.list`.
    let current = running();
    assert!(
        sidebar.contains(&format!("backlot {current}")),
        "Local's version line — {sidebar}"
    );
    assert!(
        sidebar.contains("hc 0.1.0"),
        "host-b's hc version — {sidebar}"
    );
    assert!(
        sidebar.contains(&format!("backlot {}", older())),
        "host-b's doc version — {sidebar}"
    );
}

#[tokio::test]
async fn a_behind_machine_marks_the_upgrade_hint() {
    let mut app = versioned_app(&older());
    app.update_latest = Some(running());
    app.area = (120, 36);
    let backend = draw_to(&mut app, 120, 36);
    let sidebar = sidebar_text(&backend);
    let host = sidebar.find("host-b").expect("host-b row");
    let hint = sidebar[host..]
        .find(&format!("↑{}", running()))
        .expect("the hint row");
    assert!(hint < 120, "the hint sits under host-b — {sidebar}");
}

#[tokio::test]
async fn a_machine_without_backlot_says_so() {
    let mut app = two_machine_studio_app();
    // The doc has no `backlot_version` (an old stage), and the
    // probe's locate answered `not installed`.
    app.on_mv_reply(crate::app::machine_versions::MachineProbe::Version {
        machine: "host-b".into(),
        out: Err("backlot is not installed on host-b".into()),
    });
    app.area = (120, 36);
    let backend = draw_to(&mut app, 120, 36);
    let sidebar = sidebar_text(&backend);
    assert!(sidebar.contains("no backlot"), "{sidebar}");
    // host-b's own line carries no version — only `no backlot`.
    let host = sidebar.find("host-b").expect("host-b row");
    let line = &sidebar[host..][..60];
    assert!(line.contains("no backlot"), "{line}");
    assert!(!line.contains("backlot 0."), "{line}");
}

#[tokio::test]
async fn the_sidebar_line_survives_at_80x24() {
    // The 26-cell sidebar keeps `hc` + the backlot fact; the row
    // elision itself is covered in `endpoint_sidebar_row::tests`.
    // At 80x24 the cast takes the machines tree's spare height, so
    // host-b's sub-line is elided and Local's is the fact on screen.
    let mut app = versioned_app(&older());
    app.area = (80, 24);
    let backend = draw_to(&mut app, 80, 24);
    let rows = screen_lines(&backend);
    assert!(
        rows.iter()
            .any(|r| r.contains(&format!("backlot {}", running()))),
        "the backlot fact survives — {rows:?}"
    );
}

#[tokio::test]
async fn plain_mode_has_no_version_lines() {
    // `--plain` never builds the lines — the machine rows stay one
    // line tall and the tree is herdr's, cell for cell (the
    // `EXPECTED_PLAIN` draw in studio_machines covers the rows).
    let mut app = versioned_app(&older());
    app.set_plain(true);
    app.area = (120, 24);
    let backend = draw_to(&mut app, 120, 24);
    let sidebar = sidebar_text(&backend);
    assert!(
        !sidebar.contains(&format!("backlot {}", running())),
        "{sidebar}"
    );
    assert!(
        !sidebar.contains(&format!("backlot {}", older())),
        "{sidebar}"
    );
    assert!(!sidebar.contains("hc 0.1.0"), "{sidebar}");
    for hit in &app.hit_map.machines {
        assert_eq!(hit.rect.height, 1, "{sidebar}");
    }
}

#[tokio::test]
async fn the_machine_menu_offers_its_valid_actions() {
    let mut app = versioned_app(&older());
    // Installed + behind latest + stage running → upgrade, restart,
    // new studio, then the endpoint ops.
    app.update_latest = Some(running());
    let menu = app.machine_menu(&remote("host-b"));
    assert_eq!(menu.kind, MenuKind::Machine);
    let items = menu.items();
    let labels: Vec<&str> = items.iter().map(|it| it.label.as_str()).collect();
    let upgrade = format!("Upgrade backlot to v{}", running());
    assert_eq!(
        labels,
        vec![
            upgrade.as_str(),
            "Restart the stage",
            "New studio here",
            "Connect machine",
            "Rename machine",
            "Remove machine",
        ],
        "{labels:?}"
    );
    assert_eq!(items[0].act, StudioAct::MachineInstall);
    assert_eq!(items[1].act, StudioAct::StageRestart);
    assert_eq!(items[2].act, StudioAct::NewStudio);
    assert!(
        items.iter().all(|it| it.dim.is_none()),
        "online — {items:?}"
    );
}

#[tokio::test]
async fn the_machine_menu_offers_install_and_start() {
    let mut app = two_machine_studio_app();
    // `no backlot`: only install + the endpoint ops are valid.
    app.on_mv_reply(crate::app::machine_versions::MachineProbe::Version {
        machine: "host-b".into(),
        out: Err("backlot is not installed on host-b".into()),
    });
    let menu = app.machine_menu(&remote("host-b"));
    assert_eq!(
        labels_of(&menu),
        vec![
            "Install backlot".to_owned(),
            "Connect machine".to_owned(),
            "Rename machine".to_owned(),
            "Remove machine".to_owned(),
        ]
    );
    // Stage down + installed → `Start the stage`, not restart.
    app.on_mv_reply(crate::app::machine_versions::MachineProbe::Version {
        machine: "host-b".into(),
        out: Ok(format!("backlot {}\n", older())),
    });
    let terms: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .filter(|t| t.address != "host-b/t9")
        .cloned()
        .collect();
    app.view.inventory_mut().replace(2, terms);
    let menu = app.machine_menu(&remote("host-b"));
    let labels = labels_of(&menu);
    assert!(labels.contains(&"Start the stage".to_owned()), "{labels:?}");
    assert!(!labels.iter().any(|l| l.contains("Restart")), "{labels:?}");
    assert!(labels.contains(&"New studio here".to_owned()), "{labels:?}");
}

#[tokio::test]
async fn an_offline_machine_dims_its_command_rows() {
    let mut app = two_machine_studio_app();
    app.view.machines = Some(vec![machine("host-b", "unreachable")]);
    // A probed-installed machine still lists its command rows — all
    // dimmed `unreachable` except the endpoint repair ops.
    app.machine_versions
        .note_reply("host-b", Ok(format!("backlot {}", older())));
    let menu = app.machine_menu(&remote("host-b"));
    assert!(menu.offline);
    let items = menu.items();
    for it in &items {
        if matches!(it.act, StudioAct::MachineRename | StudioAct::MachineRemove) {
            assert!(it.dim.is_none(), "endpoint ops stay live — {it:?}");
        } else {
            assert_eq!(it.dim.as_deref(), Some("unreachable"), "{it:?}");
        }
    }
    let labels: Vec<&str> = items.iter().map(|it| it.label.as_str()).collect();
    assert!(
        labels.contains(&"Restart the stage"),
        "the rows still list — {labels:?}"
    );
}

#[tokio::test]
async fn right_click_on_a_machine_opens_its_menu() {
    let mut app = versioned_app(&older());
    app.area = (120, 36);
    draw_to(&mut app, 120, 36);
    let hit = app
        .hit_map
        .machines
        .iter()
        .find(|h| h.endpoint_id == remote("host-b"))
        .map(|h| h.rect)
        .expect("host-b's machine hit");
    // The hit covers both rows — the version line's click is the
    // same machine's menu.
    assert_eq!(hit.height, 2);
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Right),
        hit.x + 1,
        hit.y + 1,
    ));
    let Some(menu) = &app.menu else {
        panic!("the machine menu opened");
    };
    let crate::chrome::menu::ContextMenuTarget::Studio(m) = &menu.target else {
        panic!("a Studio menu target — {:?}", menu.target);
    };
    assert_eq!(m.kind, MenuKind::Machine);
    assert_eq!(m.machine.as_deref(), Some("host-b"));
}

#[tokio::test]
async fn a_stage_down_row_click_starts_the_stage() {
    // `Row::StageDown` carries the machine — its target starts it.
    let row = crate::panel::Row::StageDown {
        machine: Some("host-b"),
    };
    assert_eq!(
        row.target(),
        crate::panel::Target::Stage(Some("host-b".into()))
    );
    let mut app = two_machine_studio_app();
    app.area = (120, 24);
    // Drop host-b's stage terminal — its doc stays, the stage is down.
    let terms: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .filter(|t| t.address != "host-b/t9")
        .cloned()
        .collect();
    app.view.inventory_mut().replace(2, terms);
    app.reconcile_published_view();
    // Ctrl+B y opens the dock; the `stage not running` row is a hit.
    key(&mut app, hc_vt::Key::Char('b'), hc_vt::Mods::CTRL);
    key(&mut app, hc_vt::Key::Char('y'), hc_vt::Mods::NONE);
    draw_to(&mut app, 120, 24);
    let (rect, target) = app
        .panel_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, crate::panel::Target::Stage(Some(m)) if m == "host-b"))
        .cloned()
        .expect("the stage-not-running row");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 2,
        rect.y,
    ));
    assert_eq!(app.panel.selected, Some(target), "the click selects");
    assert!(
        app.actions.pending.iter().any(|p| {
            p.action.argv == ["stage", "start"] && p.action.machine.as_deref() == Some("host-b")
        }),
        "stage start on host-b — {:?}",
        app.actions
            .pending
            .iter()
            .map(|p| &p.action.argv)
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn the_stage_restart_chains_stop_then_start() {
    let mut app = two_machine_studio_app();
    app.machine_stage_restart(Some("host-b".into()));
    let stop = app
        .actions
        .pending
        .iter()
        .find(|p| p.action.name == "stop stage")
        .expect("the stop half is pending")
        .clone();
    assert_eq!(stop.action.argv, ["stage", "stop"]);
    assert_eq!(stop.action.machine.as_deref(), Some("host-b"));
    // Its `Ok` chains `stage start` on the same machine.
    app.on_action_reply((stop.id, Ok(String::new())));
    let start = app
        .actions
        .pending
        .iter()
        .find(|p| p.action.name == "start stage")
        .expect("the start half followed");
    assert_eq!(start.action.argv, ["stage", "start"]);
    assert_eq!(start.action.machine.as_deref(), Some("host-b"));
    // A failed stop never chains.
    // Retire the chained start, then a failed stop never chains a
    // new one.
    app.on_action_reply((start.id, Ok(String::new())));
    app.machine_stage_restart(Some("host-b".into()));
    let stop = app
        .actions
        .pending
        .iter()
        .find(|p| p.action.name == "stop stage")
        .expect("stop pending again")
        .clone();
    app.on_action_reply((stop.id, Err("boom".into())));
    assert!(
        !app.actions
            .pending
            .iter()
            .any(|p| p.action.name == "start stage"),
        "no chained start after a failed stop"
    );
}

#[tokio::test]
async fn a_managed_remote_backlot_refuses_the_install() {
    let mut app = two_machine_studio_app();
    app.set_remote_runner(dead_remote_runner());
    app.machine_install(Some("host-b".into()));
    assert!(app.install_env.contains("host-b"), "the env probe spawned");
    app.on_mv_reply(crate::app::machine_versions::MachineProbe::InstallEnv {
        machine: "host-b".into(),
        out: Ok(format!(
            "EXE=/home/user/.local/share/mise/installs/backlot/{}/bin/backlot\n\
             ENV HOME=/home/user\n\
             ENV MISE_DATA_DIR=/home/user/.local/share/mise\n",
            older()
        )),
    });
    // The refusal names the channel's own command — no popup, no
    // installer line over a mise-managed binary.
    assert!(app.notice.is_some(), "the refusal notice is up");
    assert!(app.popup_pending.is_none(), "no install popup spawned");
    assert!(!app.install_env.contains("host-b"));
    app.area = (120, 36);
    let backend = draw_to(&mut app, 120, 36);
    let screen = screen_text(&backend);
    assert!(
        screen.contains("mise upgrade --bump backlot"),
        "the channel's own command — {screen}"
    );
}

#[tokio::test]
async fn a_direct_remote_install_builds_the_pinned_popup() {
    let mut app = versioned_app(&older());
    app.set_remote_runner(dead_remote_runner());
    app.update_latest = Some(running());
    app.machine_install(Some("host-b".into()));
    app.on_mv_reply(crate::app::machine_versions::MachineProbe::InstallEnv {
        machine: "host-b".into(),
        out: Ok("EXE=/home/user/.local/bin/backlot\nENV HOME=/home/user\n".into()),
    });
    // A direct install plans the popup on host-b — the pinned,
    // skip-core installer line is `machine_install::install_line`'s
    // own tested contract.
    let pending = app
        .popup_pending
        .as_ref()
        .expect("the install popup is pending");
    assert_eq!(pending.install_machine.as_deref(), Some("host-b"));
    // host-b's stage runs (t9) — the post-install restart
    // comparison armed on the pre-install version.
    assert_eq!(
        app.install_upgrade.get("host-b"),
        Some(&Some(older())),
        "armed with the doc's version"
    );
}

#[tokio::test]
async fn the_install_popup_close_reprobes_the_machine() {
    let mut app = two_machine_studio_app();
    app.machine_versions
        .note_reply("host-b", Ok(format!("backlot {}", older())));
    // The doc still says the old version — the post-install probe must
    // run anyway (the doc is the old stage's word).
    app.install_upgrade.insert("host-b".into(), Some(older()));
    app.machine_install_closed("host-b");
    assert_eq!(
        app.machine_versions.state("host-b", None),
        Backlot::Probing,
        "the close invalidated + re-probed"
    );
}

#[tokio::test]
async fn a_bumped_version_after_install_restarts_the_stage() {
    let mut app = versioned_app(&older());
    app.install_upgrade.insert("host-b".into(), Some(older()));
    // The install popup's close re-probed; the machine answers the
    // running release.
    app.on_mv_reply(crate::app::machine_versions::MachineProbe::Version {
        machine: "host-b".into(),
        out: Ok(format!("backlot {}\n", running())),
    });
    // host-b's stage runs — the bumped version chains the restart.
    assert!(
        app.actions
            .pending
            .iter()
            .any(|p| p.action.name == "stop stage"
                && p.action.machine.as_deref() == Some("host-b")),
        "the restart's stop half is pending — {:?}",
        app.actions
            .pending
            .iter()
            .map(|p| &p.action.name)
            .collect::<Vec<_>>()
    );
    assert!(!app.install_upgrade.contains_key("host-b"));
    // An unchanged version never restarts.
    let mut app = versioned_app(&running());
    app.install_upgrade.insert("host-b".into(), Some(running()));
    app.on_mv_reply(crate::app::machine_versions::MachineProbe::Version {
        machine: "host-b".into(),
        out: Ok(format!("backlot {}\n", running())),
    });
    assert!(
        !app.actions
            .pending
            .iter()
            .any(|p| p.action.name == "stop stage"),
        "no restart when nothing changed"
    );
}

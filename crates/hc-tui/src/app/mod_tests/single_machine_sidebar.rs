//! The machine-first sidebar with a single endpoint: studio mode always
//! draws the federated machine tree, even when only the local machine is
//! present, while `--plain` keeps herdr's spaces sidebar. Neutral
//! fixtures only (`acme-shop`, `backlot`).

use super::endpoint_nav::pending_attach;
use super::*;
use crate::app::input::{MouseButton, MouseEventKind, MouseInput};
use crate::chrome::navigator::NavigatorTarget;
use crate::input::KeybindAction as A;

/// One endpoint (`Local`), two workspaces, studio mode on.
fn studio_app_one_machine() -> App {
    let mut app = app_with_spaces(&[("t1", "acme-shop"), ("t2", "beta-tools")]);
    app.set_plain(false);
    app.focused = Some("t1".into());
    app.refresh_projection();
    // The Hollywood bar would share the top rows these tests read.
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

fn workspace_id(app: &App, label: &str) -> String {
    app.projection.endpoints[0]
        .snapshot
        .as_deref()
        .and_then(|snapshot| {
            snapshot
                .workspaces
                .iter()
                .find(|workspace| workspace.label == label)
        })
        .map(|workspace| workspace.workspace_id.clone())
        .unwrap_or_else(|| panic!("workspace {label} is projected"))
}

#[tokio::test]
async fn studio_mode_with_one_machine_draws_the_machine_tree() {
    let mut app = studio_app_one_machine();
    app.area = (120, 24);
    let backend = draw_to(&mut app, 120, 24);
    let sidebar = sidebar_text(&backend);
    assert!(sidebar.contains("machines"), "{sidebar}");
    assert!(sidebar.contains("Local"), "{sidebar}");
    // The workspaces nest under the `Local` row, indented like the
    // federated tree's children.
    let lines = sidebar.lines().collect::<Vec<_>>();
    let machine = lines
        .iter()
        .position(|line| line.contains("Local"))
        .expect("the machine row");
    let shop = lines
        .iter()
        .position(|line| line.contains("acme-shop"))
        .expect("the acme-shop workspace row");
    assert!(machine < shop, "workspaces follow the machine:\n{sidebar}");
    assert!(
        lines[shop].starts_with("   "),
        "the workspace row is indented under the machine:\n{sidebar}"
    );
    // The hit map carries the machine row and both workspace rows.
    assert_eq!(app.hit_map.machines.len(), 1, "{sidebar}");
    assert_eq!(app.hit_map.machines[0].endpoint_id, ClientEndpointId::Local);
    assert_eq!(app.hit_map.workspaces.len(), 2, "{sidebar}");
}

#[tokio::test]
async fn plain_mode_with_one_machine_keeps_the_spaces_sidebar() {
    let mut app = app_with_spaces(&[("t1", "acme-shop"), ("t2", "backlot")]);
    app.set_plain(true);
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.area = (120, 24);
    let backend = draw_to(&mut app, 120, 24);
    let sidebar = sidebar_text(&backend);
    // herdr's single-endpoint fallback: the spaces list, no machine
    // rows, no federated hit map.
    assert!(sidebar.contains("spaces"), "{sidebar}");
    assert!(sidebar.contains("acme-shop"), "{sidebar}");
    assert!(!sidebar.contains("machines"), "{sidebar}");
    assert!(!sidebar.contains("Local"), "{sidebar}");
    assert!(app.hit_map.machines.is_empty(), "{sidebar}");
    assert_eq!(app.hit_map.workspaces.len(), 2, "{sidebar}");
}

#[tokio::test]
async fn workspace_nav_and_machine_clicks_work_with_one_machine() {
    let mut app = studio_app_one_machine();
    pending_attach(&mut app);
    app.area = (120, 24);
    draw_to(&mut app, 120, 24);

    // Next-workspace walks the machine tree even with one endpoint.
    app.on_action(A::NextWorkspace, 80);
    assert_eq!(app.focused.as_deref(), Some("t2"));
    assert_eq!(app.projection.active_endpoint_id, ClientEndpointId::Local);

    // The machine row toggles its section on press.
    let toggle = app.hit_map.machines[0].collapse_toggle;
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        toggle.x,
        toggle.y,
    ));
    assert!(
        app.shell_state
            .collapsed_endpoints
            .contains(&ClientEndpointId::Local),
        "the machine row collapsed"
    );
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        toggle.x,
        toggle.y,
    ));
    assert!(
        !app.shell_state
            .collapsed_endpoints
            .contains(&ClientEndpointId::Local),
        "the machine row expanded again"
    );

    // A workspace row press focuses its terminal.
    let backend = draw_to(&mut app, 120, 24);
    let _ = backend;
    let shop = workspace_id(&app, "acme-shop");
    let hit = app
        .hit_map
        .workspaces
        .iter()
        .find(|hit| hit.workspace_id == shop)
        .expect("the acme-shop workspace hit")
        .rect;
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        hit.x + 1,
        hit.y,
    ));
    app.on_mouse(mouse(
        MouseEventKind::Up(MouseButton::Left),
        hit.x + 1,
        hit.y,
    ));
    assert_eq!(app.focused.as_deref(), Some("t1"));
    assert_eq!(app.projection.active_endpoint_id, ClientEndpointId::Local);
}

#[tokio::test]
async fn studio_mode_hides_backlot_stage_space_while_plain_keeps_it() {
    let mut app = app_with_spaces(&[("t1", "acme-shop"), ("t2", "backlot"), ("t3", "beta-tools")]);
    app.set_plain(false);
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    app.area = (120, 24);
    let backend = draw_to(&mut app, 120, 24);
    let sidebar = sidebar_text(&backend);
    assert!(sidebar.contains("acme-shop"), "{sidebar}");
    assert!(sidebar.contains("beta-tools"), "{sidebar}");
    assert!(
        !sidebar.contains("· backlot"),
        "stage space hidden: {sidebar}"
    );
    assert_eq!(app.hit_map.workspaces.len(), 2, "{sidebar}");

    pending_attach(&mut app);
    app.on_action(A::NextWorkspace, 80);
    assert_eq!(app.focused.as_deref(), Some("t3"), "skips backlot space");
    app.on_action(A::NextWorkspace, 80);
    assert_eq!(
        app.focused.as_deref(),
        Some("t1"),
        "wraps past backlot space"
    );

    app.set_plain(true);
    app.refresh_projection();
    let plain_backend = draw_to(&mut app, 120, 24);
    let plain_sidebar = sidebar_text(&plain_backend);
    assert!(
        plain_sidebar.contains("backlot"),
        "visible in --plain: {plain_sidebar}"
    );
    assert_eq!(app.hit_map.workspaces.len(), 3, "{plain_sidebar}");
}

#[tokio::test]
async fn the_new_row_opens_the_dialog_with_the_local_machine() {
    let mut app = studio_app_one_machine();
    app.area = (120, 24);
    draw_to(&mut app, 120, 24);
    let rect = app.hit_map.new_workspace;
    assert!(!rect.is_empty(), "the `new · Local` row has a hit");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 1,
        rect.y,
    ));
    let dialog = app.new_studio.as_ref().expect("the dialog opened");
    assert_eq!(dialog.machine_sel, 0, "Local is preselected");
}

#[tokio::test]
async fn the_navigator_lists_the_machine_in_studio_mode() {
    let mut app = studio_app_one_machine();
    app.area = (120, 24);
    app.on_action(A::OpenNavigator, 80);
    assert!(app.navigator.is_some(), "the overlay opened");
    draw_to(&mut app, 120, 24);
    assert!(
        app.navigator_hits
            .rows
            .iter()
            .any(|(_, target)| matches!(target, NavigatorTarget::Machine { .. })),
        "the machine row is in the overlay"
    );
}

#[tokio::test]
async fn the_mobile_switcher_lists_the_machine_in_studio_mode() {
    let mut app = studio_app_one_machine();
    app.mode = Mode::Navigate;
    app.sched.mark_dirty(Instant::now());
    let mut term = Terminal::new(TestBackend::new(64, 24)).unwrap();
    app.maybe_draw(&mut term, Instant::now()).unwrap();
    let text = term
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("machines"), "{text}");
    assert!(text.contains("Local"), "{text}");
}

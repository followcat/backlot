//! Cross-endpoint navigation: with several sidebar endpoints,
//! previous/next agent and previous/next workspace cycle through every
//! Online machine (the reference's `handle_endpoint_navigation`), while
//! tab and indexed-workspace actions stay on the active endpoint.

use super::*;
use crate::input::KeybindAction as A;
use crate::sidebar::fixture::{agent, machine, term};

/// Attach machinery that never resolves — focus changes still apply.
fn pending_attach(app: &mut App) {
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
}

/// A view with remote endpoints: `host-a/t7`-style addresses plus their
/// `machine.list` rows (the state word decides Online vs stale).
fn app_multi(
    terms: Vec<hc_proto::terminal::Terminal>,
    machines: Vec<hc_proto::machine::MachineView>,
) -> App {
    let mut app = app();
    let mut view = View {
        link: hc_proto::watch::Link::Online,
        rpc: hc_proto::watch::Link::Online,
        core: Some(hc_proto::hello::CoreInfo {
            id: "id-local".into(),
            name: "local".into(),
            hostname: None,
        }),
        machines: Some(machines),
        ..Default::default()
    };
    view.inventory.replace(1, terms);
    app.view = view;
    app.refresh_projection();
    app
}

/// A live reporting agent in `space`/`tab` — an agent panel row and a
/// workspace row on its machine.
fn spaced_agent(address: &str, space: &str) -> hc_proto::terminal::Terminal {
    let mut t = agent(address, "working");
    t.labels.insert("herdr.space".into(), space.into());
    t.labels.insert("herdr.tab".into(), "tab".into());
    t
}

/// A plain terminal in `space`/`tab` — a pane and workspace row, never
/// an agent row (no status records).
fn spaced_term(address: &str, space: &str) -> hc_proto::terminal::Terminal {
    let mut t = term(address);
    t.labels.insert("herdr.space".into(), space.into());
    t.labels.insert("herdr.tab".into(), "tab".into());
    t
}

fn remote(name: &str) -> ClientEndpointId {
    ClientEndpointId::Remote(name.into())
}

/// Local agents `t1`/`t2` and online host-a agents `host-a/t3`/`t4`,
/// each in its own workspace (`one`/`two`/`three`/`four`).
fn two_machine_app() -> App {
    app_multi(
        vec![
            spaced_agent("t1", "one"),
            spaced_agent("t2", "two"),
            spaced_agent("host-a/t3", "three"),
            spaced_agent("host-a/t4", "four"),
        ],
        vec![machine("host-a", "connected")],
    )
}

#[tokio::test]
async fn next_agent_crosses_into_host_a_and_previous_returns() {
    let mut app = two_machine_app();
    pending_attach(&mut app);
    app.focused = Some("t2".into());
    app.refresh_projection();

    app.on_action(A::NextAgent, 80);
    assert_eq!(app.focused.as_deref(), Some("host-a/t3"));
    assert_eq!(app.projection.active_endpoint_id, remote("host-a"));

    app.on_action(A::PreviousAgent, 80);
    assert_eq!(app.focused.as_deref(), Some("t2"));
    assert_eq!(app.projection.active_endpoint_id, ClientEndpointId::Local);
}

#[tokio::test]
async fn previous_agent_wraps_backward_across_machines() {
    let mut app = two_machine_app();
    pending_attach(&mut app);
    app.focused = Some("t1".into());
    app.refresh_projection();

    app.on_action(A::PreviousAgent, 80);
    assert_eq!(app.focused.as_deref(), Some("host-a/t4"));
    assert_eq!(app.projection.active_endpoint_id, remote("host-a"));
}

#[tokio::test]
async fn next_agent_into_another_workspace_focuses_and_reveals_it() {
    let mut app = two_machine_app();
    pending_attach(&mut app);
    app.focused = Some("t1".into());
    app.refresh_projection();
    let ws_two = "space:3:two".to_string();
    app.shell_state.collapsed_groups.insert(ws_two.clone());

    app.on_action(A::NextAgent, 80);
    assert_eq!(app.focused.as_deref(), Some("t2"));
    assert_eq!(
        app.projection.snapshot.focused_workspace_id.as_deref(),
        Some(ws_two.as_str()),
        "the agent's workspace follows the focus"
    );
    assert!(
        !app.shell_state.collapsed_groups.contains(&ws_two),
        "a collapsed group expands to reveal the agent's workspace"
    );
    assert!(app.shell_state.reveal_focused_workspace);
}

#[tokio::test]
async fn non_online_machine_is_skipped_for_agents_and_workspaces() {
    let mut app = app_multi(
        vec![
            spaced_agent("t1", "one"),
            spaced_agent("host-a/t2", "two"),
            spaced_agent("host-b/t3", "three"),
        ],
        vec![
            machine("host-a", "connected"),
            machine("host-b", "unreachable"),
        ],
    );
    pending_attach(&mut app);
    app.focused = Some("t1".into());
    app.refresh_projection();

    // host-b is Attention, never Online: its agents never win either
    // direction of the cycle.
    app.on_action(A::NextAgent, 80);
    assert_eq!(app.focused.as_deref(), Some("host-a/t2"));
    app.on_action(A::NextAgent, 80);
    assert_eq!(app.focused.as_deref(), Some("t1"));
    app.on_action(A::PreviousAgent, 80);
    assert_eq!(app.focused.as_deref(), Some("host-a/t2"));

    app.focused = Some("t1".into());
    app.refresh_projection();
    app.on_action(A::NextWorkspace, 80);
    assert_eq!(
        app.projection.snapshot.focused_workspace_id.as_deref(),
        Some("space:3:two")
    );
    app.on_action(A::NextWorkspace, 80);
    assert_eq!(
        app.projection.snapshot.focused_workspace_id.as_deref(),
        Some("space:3:one"),
        "workspace nav wraps past the offline machine's rows"
    );
    assert_eq!(app.focused.as_deref(), Some("t1"));
}

#[tokio::test]
async fn next_workspace_crosses_and_previous_wraps_across_machines() {
    let mut app = two_machine_app();
    pending_attach(&mut app);
    app.focused = Some("t2".into());
    app.refresh_projection();

    app.on_action(A::NextWorkspace, 80);
    assert_eq!(
        app.projection.snapshot.focused_workspace_id.as_deref(),
        Some("space:5:three")
    );
    assert_eq!(app.focused.as_deref(), Some("host-a/t3"));
    assert_eq!(app.projection.active_endpoint_id, remote("host-a"));

    app.focused = Some("t1".into());
    app.refresh_projection();
    app.on_action(A::PreviousWorkspace, 80);
    assert_eq!(
        app.projection.snapshot.focused_workspace_id.as_deref(),
        Some("space:4:four")
    );
    assert_eq!(app.focused.as_deref(), Some("host-a/t4"));
}

#[tokio::test]
async fn collapsed_endpoint_or_group_is_reached_and_expanded() {
    let mut app = two_machine_app();
    pending_attach(&mut app);
    app.shell_state.collapsed_endpoints.insert(remote("host-a"));
    app.shell_state
        .remote_collapsed_groups
        .entry(remote("host-a"))
        .or_default()
        .insert("space:5:three".to_string());
    app.focused = Some("t2".into());
    app.refresh_projection();

    app.on_action(A::NextWorkspace, 80);
    assert_eq!(app.focused.as_deref(), Some("host-a/t3"));
    assert!(
        !app.shell_state
            .collapsed_endpoints
            .contains(&remote("host-a")),
        "the endpoint expands to show the focused workspace"
    );
    assert!(
        !app.shell_state
            .remote_collapsed_groups
            .get(&remote("host-a"))
            .is_some_and(|groups| groups.contains("space:5:three")),
        "the workspace's collapsed group expands"
    );
}

#[tokio::test]
async fn no_current_position_next_picks_first_previous_picks_last() {
    let mut app = two_machine_app();
    pending_attach(&mut app);

    app.focused = None;
    app.refresh_projection();
    app.on_action(A::NextWorkspace, 80);
    assert_eq!(
        app.projection.snapshot.focused_workspace_id.as_deref(),
        Some("space:3:one"),
        "Next with no position selects the first workspace"
    );
    assert_eq!(app.focused.as_deref(), Some("t1"));

    app.focused = None;
    app.refresh_projection();
    app.on_action(A::PreviousWorkspace, 80);
    assert_eq!(
        app.projection.snapshot.focused_workspace_id.as_deref(),
        Some("space:4:four"),
        "Previous with no position selects the last workspace"
    );
    assert_eq!(app.focused.as_deref(), Some("host-a/t4"));
}

#[tokio::test]
async fn agent_nav_without_a_current_agent_starts_at_the_ends() {
    let mut app = app_multi(
        vec![
            spaced_term("t9", "shell"),
            spaced_agent("t1", "one"),
            spaced_agent("host-a/t2", "two"),
        ],
        vec![machine("host-a", "connected")],
    );
    pending_attach(&mut app);
    // The focused pane is no agent row — the cycle has no position.
    app.focused = Some("t9".into());
    app.refresh_projection();

    app.on_action(A::NextAgent, 80);
    assert_eq!(app.focused.as_deref(), Some("t1"));
    app.focused = Some("t9".into());
    app.refresh_projection();
    app.on_action(A::PreviousAgent, 80);
    assert_eq!(app.focused.as_deref(), Some("host-a/t2"));
}

#[tokio::test]
async fn focus_agent_indexes_the_aggregate_order() {
    let mut app = two_machine_app();
    pending_attach(&mut app);
    app.focused = Some("t1".into());
    app.refresh_projection();

    app.on_action(A::FocusAgent(3), 80);
    assert_eq!(app.focused.as_deref(), Some("host-a/t4"));
    assert_eq!(app.projection.active_endpoint_id, remote("host-a"));

    app.on_action(A::FocusAgent(0), 80);
    assert_eq!(app.focused.as_deref(), Some("t1"));
    assert_eq!(app.projection.active_endpoint_id, ClientEndpointId::Local);
}

#[tokio::test]
async fn focus_agent_out_of_range_is_a_no_op() {
    let mut app = two_machine_app();
    pending_attach(&mut app);
    app.focused = Some("t2".into());
    app.refresh_projection();

    app.on_action(A::FocusAgent(9), 80);
    assert_eq!(app.focused.as_deref(), Some("t2"));
    assert_eq!(app.projection.active_endpoint_id, ClientEndpointId::Local);
}

#[tokio::test]
async fn agent_row_reveal_uses_the_rendered_index_and_keeps_visible_rows() {
    // A stale machine's rows still render above the target: the scroll
    // lands on the row's position in the full aggregate list.
    let mut app = app_multi(
        vec![
            spaced_agent("t1", "one"),
            spaced_agent("host-a/t2", "two"),
            spaced_agent("host-b/t3", "three"),
        ],
        vec![
            machine("host-a", "unreachable"),
            machine("host-b", "connected"),
        ],
    );
    pending_attach(&mut app);
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.hit_map.agent_max_scroll = 50;
    app.hit_map.agents.clear();
    app.hit_map.endpoint_agents.clear();

    app.on_action(A::NextAgent, 80);
    assert_eq!(app.focused.as_deref(), Some("host-b/t3"));
    assert_eq!(
        app.shell_state.agent_scroll, 2,
        "host-b/t3 is the third rendered row (host-a's stale row above)"
    );

    // A row already on-screen keeps the scroll offset untouched.
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.shell_state.agent_scroll = 7;
    app.hit_map.endpoint_agents.push((
        ratatui::layout::Rect::default(),
        remote("host-b"),
        "host-b/t3".into(),
    ));
    app.on_action(A::NextAgent, 80);
    assert_eq!(app.focused.as_deref(), Some("host-b/t3"));
    assert_eq!(app.shell_state.agent_scroll, 7);
}

#[tokio::test]
async fn next_tab_stays_in_the_workspace() {
    // Two same-space terminals land in distinct tabs of workspace `one`.
    let mut t2 = spaced_agent("t2", "one");
    t2.labels.insert("herdr.tab".into(), "other".into());
    let mut app = app_multi(
        vec![
            spaced_agent("t1", "one"),
            t2,
            spaced_agent("host-a/t3", "three"),
        ],
        vec![machine("host-a", "connected")],
    );
    pending_attach(&mut app);
    app.focused = Some("t1".into());
    app.refresh_projection();
    let ws_one = app.projection.snapshot.focused_workspace_id.clone();

    app.on_action(A::NextTab, 80);
    assert_eq!(app.focused.as_deref(), Some("t2"));
    assert_eq!(app.projection.snapshot.focused_workspace_id, ws_one);
    assert_eq!(app.projection.active_endpoint_id, ClientEndpointId::Local);

    app.on_action(A::NextTab, 80);
    assert_eq!(
        app.focused.as_deref(),
        Some("t1"),
        "tabs wrap within the workspace"
    );
}

#[tokio::test]
async fn switch_workspace_stays_on_the_active_endpoint() {
    let mut app = app_multi(
        vec![
            spaced_agent("t1", "one"),
            spaced_agent("t2", "two"),
            spaced_agent("host-a/t3", "three"),
        ],
        vec![machine("host-a", "connected")],
    );
    pending_attach(&mut app);
    app.focused = Some("t1".into());
    app.refresh_projection();

    // Index 1 names the second workspace of the ACTIVE endpoint only.
    app.on_action(A::SwitchWorkspace(1), 80);
    assert_eq!(
        app.projection.snapshot.focused_workspace_id.as_deref(),
        Some("space:3:two")
    );
    assert_eq!(app.focused.as_deref(), Some("t2"));
    assert_eq!(app.projection.active_endpoint_id, ClientEndpointId::Local);

    // Out of the active endpoint's range: nothing moves.
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.on_action(A::SwitchWorkspace(5), 80);
    assert_eq!(app.focused.as_deref(), Some("t1"));
    assert_eq!(app.projection.active_endpoint_id, ClientEndpointId::Local);
}

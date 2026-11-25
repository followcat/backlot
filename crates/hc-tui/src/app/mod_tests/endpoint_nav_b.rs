use super::endpoint_nav::*;
use super::*;
use crate::input::KeybindAction as A;
use crate::sidebar::fixture::machine;

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

#[tokio::test]
async fn agent_row_reveal_counts_space_headers_across_machines() {
    // Headers open each machine's space: [one] t1, [two] host-a/t2,
    // [three] host-b/t3 — host-b/t3 is the sixth rendered entry.
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
    app.shell.agent_panel_sort = crate::config::AgentPanelSortConfig::Spaces;
    app.shell.agent_panel_space_headers = true;
    pending_attach(&mut app);
    app.focused = Some("t1".into());
    app.refresh_projection();
    app.hit_map.agent_max_scroll = 50;
    app.hit_map.agents.clear();
    app.hit_map.endpoint_agents.clear();

    app.on_action(A::NextAgent, 80);
    assert_eq!(app.focused.as_deref(), Some("host-b/t3"));
    assert_eq!(app.shell_state.agent_scroll, 5);
}

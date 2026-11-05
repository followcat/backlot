use super::*;

#[test]
fn delimiter_bearing_labels_keep_distinct_tab_identity_and_focus() {
    let screen = snapshot(&[labeled("t1", "a/tab:b", "c"), labeled("t2", "a", "b/tab:c")]);
    assert_eq!(screen.tabs.len(), 2);
    assert_ne!(screen.tabs[0].tab_id, screen.tabs[1].tab_id);
    for pane in &screen.panes {
        let tab = screen
            .tabs
            .iter()
            .find(|t| t.tab_id == pane.tab_id)
            .unwrap();
        assert_eq!(tab.workspace_id, pane.workspace_id);
    }
    let focused = screen.tabs.iter().find(|tab| tab.focused).unwrap();
    assert_eq!(screen.focused_tab_id.as_ref(), Some(&focused.tab_id));
}
#[test]
fn an_explicit_default_label_does_not_merge_with_the_unlabelled_tab() {
    let mut unlabelled = term("t1");
    unlabelled
        .labels
        .insert("herdr.space".into(), "acme-shop".into());
    let screen = snapshot(&[unlabelled, labeled("t2", "acme-shop", "default")]);
    assert_eq!(screen.tabs.len(), 2);
    assert_ne!(screen.tabs[0].tab_id, screen.tabs[1].tab_id);
}
#[test]
fn unlabelled_tab_uses_its_number_instead_of_the_workspace_label() {
    let mut terminal = term("t1");
    terminal
        .labels
        .insert("herdr.space".into(), "acme-shop".into());
    let screen = snapshot(&[terminal]);
    assert_eq!(screen.tabs[0].label, "1");
    assert!(!screen.tabs[0].custom_label);
}
#[test]
fn manual_reported_and_terminal_titles_keep_their_distinct_meanings() {
    let mut terminal = term("t1");
    terminal.name = Some("alice".into());
    terminal
        .labels
        .insert("herdr.title".into(), "manual-label".into());
    terminal.title = "osc-title".into();
    terminal.reported = true;
    terminal.status = Some(Status {
        state: "working".into(),
        app: "agent-a".into(),
        title: "reported-title".into(),
        ..Status::default()
    });
    let screen = snapshot(&[terminal]);
    assert_eq!(screen.panes[0].label.as_deref(), Some("manual-label"));
    assert_eq!(screen.agents[0].name.as_deref(), Some("alice"));
    assert_eq!(screen.agents[0].title.as_deref(), Some("reported-title"));
    assert_eq!(
        screen.agents[0].terminal_title.as_deref(),
        Some("osc-title")
    );
}
#[test]
fn simultaneous_creation_uses_terminal_number_before_label_order() {
    let screen = snapshot(&[
        labeled("t10", "alpha", "alpha"),
        labeled("t2", "zulu", "zulu"),
    ]);
    assert_eq!(screen.workspaces[0].label, "zulu");
    let screen = snapshot(&[
        labeled("t10", "acme-shop", "alpha"),
        labeled("t2", "acme-shop", "zulu"),
    ]);
    assert_eq!(screen.tabs[0].label, "zulu");
}
#[test]
fn priority_order_uses_status_recency_instead_of_per_terminal_turn_count() {
    let mut older = term("t1");
    older.reported = true;
    older.status = Some(Status {
        state: "blocked".into(),
        ..Status::default()
    });
    older.turn = 90;
    older.status_ms = Some(1_000);
    let mut newer = older.clone();
    newer.address = "t2".into();
    newer.turn = 1;
    newer.status_ms = Some(2_000);
    let screen = snapshot(&[older, newer]);
    let ordered = crate::chrome::agents::ordered_agent_pane_ids(
        &screen,
        crate::config::AgentPanelSortConfig::Priority,
    );
    assert_eq!(ordered, ["t2", "t1"]);
}
#[test]
fn aggregate_priority_retains_the_endpoint_recency_map() {
    let mut local = term("t1");
    local.reported = true;
    local.status = Some(Status {
        state: "blocked".into(),
        ..Status::default()
    });
    let mut remote = local.clone();
    remote.address = "host-b/t2".into();
    let mut view = View::default();
    view.inventory.replace(1, vec![local, remote]);
    let mut projected = project(&view, Some("t1"));
    for endpoint in &mut projected.endpoints {
        endpoint.status = ClientEndpointStatus::Online;
        let pane = &endpoint.snapshot.as_ref().unwrap().agents[0].pane_id;
        let recency = if endpoint.endpoint_id.is_local() {
            1
        } else {
            2
        };
        endpoint.agent_recency.insert(pane.clone(), recency);
    }
    let ordered = crate::chrome::endpoints::aggregate_agent_rows(
        &projected.endpoints,
        &projected.active_endpoint_id,
        crate::config::AgentPanelSortConfig::Priority,
    );
    assert_eq!(ordered[0].agent.pane_id, "host-b/t2");
}
#[test]
fn spaces_group_by_label_then_cwd() {
    let mut labeled = agent("t1", "working");
    labeled.labels.insert("herdr.space".into(), "webapp".into());
    let mut other_dir = agent("t2", "idle");
    other_dir.cwd = "/home/user/api".into();
    let mut same_base = agent("t3", "idle");
    same_base.cwd = "/opt/proj".into();
    // `term("t4")` defaults to cwd `/home/user/proj`.
    let view = view(vec![labeled, other_dir, same_base, term("t4")], vec![]);
    let projected = project(&view, Some("t1"));
    let snapshot = &projected.snapshot;

    let labels: Vec<&str> = snapshot
        .workspaces
        .iter()
        .map(|w| w.label.as_str())
        .collect();
    assert!(labels.contains(&"webapp"), "{labels:?}");
    assert!(labels.contains(&"api"), "{labels:?}");
    // Two different cwds sharing the `proj` basename stay two spaces.
    assert_eq!(
        snapshot
            .workspaces
            .iter()
            .filter(|w| w.label == "proj")
            .count(),
        2,
        "{labels:?}"
    );
    assert_eq!(
        snapshot
            .workspaces
            .iter()
            .map(|w| &w.workspace_id)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        4,
        "distinct workspace identities"
    );
}
#[test]
fn machines_keep_offline_state_and_inventory() {
    let mut down = machine("host-b", "unreachable");
    down.why = Some("ssh: connection refused".into());
    let view = view(
        vec![agent("host-b/t7", "blocked"), agent("t1", "done")],
        vec![down],
    );
    let projected = project(&view, Some("t1"));

    let host_b = endpoint(&projected, "host-b");
    // `unreachable` is not a live word — the endpoint shows attention
    // (the reference `stale() == !Online` rule dims its agents below).
    assert_eq!(host_b.status, ClientEndpointStatus::Attention);
    let snapshot = host_b.snapshot.as_deref().unwrap();
    assert!(
        snapshot.panes.iter().any(|p| p.pane_id == "host-b/t7"),
        "offline machine keeps its retained inventory"
    );
    assert_eq!(
        projected.endpoints.len(),
        2,
        "local + host-b endpoints both present"
    );
    assert_eq!(
        endpoint(&projected, "local").status,
        ClientEndpointStatus::Online
    );
}
#[test]
fn each_live_agent_appears_once_and_shells_never_do() {
    let mut worker = agent("t9", "working");
    worker.parent = Some("t1".into());
    let view = view(
        vec![
            agent("t1", "blocked"),
            agent("t2", "working"),
            agent("t3", "done"),
            agent("t4", "idle"),
            worker,
            term("t5"), // a shell: reported never set
        ],
        vec![],
    );
    let projected = project(&view, Some("t1"));
    let agents = &projected.snapshot.agents;
    let mut ids: Vec<&str> = agents.iter().map(|a| a.pane_id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["t1", "t2", "t3", "t4", "t9"], "agents once each");
    assert!(
        agents.iter().all(|a| a.pane_id != "t5"),
        "shells are never agents"
    );
}
#[test]
fn agents_order_blocked_first_with_deterministic_ties() {
    // The reference priority is Blocked > Done > Working > Idle —
    // Raw idle stays idle; client completion policy is tested separately.
    // Ties break on observed
    // status-change recency (`status_ms`), then creation order.
    let mut t1 = agent("t1", "idle");
    let mut t2 = agent("t2", "blocked");
    let mut t3 = agent("t3", "done");
    let mut remote = agent("host-b/t4", "blocked");
    let mut newer = agent("t5", "working");
    t2.status_ms = Some(1);
    remote.status_ms = Some(2);
    for (i, t) in [&mut t1, &mut t2, &mut t3, &mut remote, &mut newer]
        .iter_mut()
        .enumerate()
    {
        t.created_ms = i as u64;
    }
    let view = view(
        vec![t1, t2, t3, remote, newer, term("t9")],
        vec![machine("host-b", "connected")],
    );
    let projected = project(&view, Some("t5"));
    let ordered = crate::chrome::endpoints::aggregate_agent_rows(
        &projected.endpoints,
        &projected.active_endpoint_id,
        crate::config::AgentPanelSortConfig::Priority,
    );
    let order: Vec<&str> = ordered.iter().map(|r| r.agent.pane_id.as_str()).collect();
    assert_eq!(
        order,
        ["host-b/t4", "t2", "t3", "t5", "t1"],
        "blocked by recency, then done, working; shells excluded"
    );
}
#[test]
fn remote_worker_keeps_its_own_qualified_identity() {
    // Core localizes `parent`: this remote worker was started by local
    // t2 — the parent never rewrites the worker's own endpoint fields.
    let mut worker = agent("host-b/t9", "working");
    worker.parent = Some("t2".into());
    let mut launcher = agent("t2", "idle");
    launcher.name = Some("boss".into());
    let view = view(vec![worker, launcher], vec![machine("host-b", "connected")]);
    let projected = project(&view, Some("t2"));

    let host_b = endpoint(&projected, "host-b");
    let snapshot = host_b.snapshot.as_deref().unwrap();
    let row = snapshot
        .agents
        .iter()
        .find(|a| a.pane_id == "host-b/t9")
        .expect("remote worker keeps its qualified address");
    // No parent presentation exists in the reference snapshot — the
    // worker's own fields are all that appear, unprefixed by `t2`.
    for field in [&row.name, &row.display_agent, &row.title]
        .into_iter()
        .flatten()
    {
        assert!(!field.contains("t2"), "parent leaked into {field:?}");
    }
    assert!(
        snapshot.panes.iter().any(|p| p.pane_id == "host-b/t9"),
        "the worker is a pane in its own endpoint"
    );
}
#[test]
fn an_unknown_parent_does_not_hide_the_worker() {
    let mut worker = agent("t5", "working");
    worker.parent = Some("host-c/t1".into());
    let view = view(vec![worker], vec![]);
    let projected = project(&view, Some("t5"));
    assert!(
        projected.snapshot.agents.iter().any(|a| a.pane_id == "t5"),
        "a worker with an unknown parent still lists"
    );
}

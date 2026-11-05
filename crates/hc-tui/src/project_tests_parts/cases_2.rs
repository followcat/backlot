use super::*;

#[test]
fn unknown_words_and_records_map_through_reference_status() {
    // The custom `word·name`/`·source` badge suffixes are gone — the
    // effective record decides `agent_status`, unknown words fall to
    // `Unknown`, and no presentation carries the record name or source.
    let mut odd = agent("t1", "negotiating");
    odd.status
        .as_mut()
        .unwrap()
        .extra
        .insert("source".into(), serde_json::Value::String("screen".into()));
    let mut named = agent("t2", "working");
    named.records.insert("sub:build".into(), status("blocked"));
    let view = view(vec![odd, named], vec![]);
    let projected = project(&view, Some("t1"));
    let agents = &projected.snapshot.agents;

    let odd_row = agents.iter().find(|a| a.pane_id == "t1").unwrap();
    assert_eq!(odd_row.agent_status, AgentStatus::Unknown);
    let named_row = agents.iter().find(|a| a.pane_id == "t2").unwrap();
    // The named `blocked` record outranks the root `working` — the
    // reference's most-urgent-effective rule.
    assert_eq!(named_row.agent_status, AgentStatus::Blocked);
    for row in agents {
        for field in [&row.name, &row.display_agent, &row.title]
            .into_iter()
            .flatten()
        {
            assert!(
                !field.contains('·') && !field.contains("screen") && !field.contains("sub:build"),
                "custom suffix leaked into {field:?}"
            );
        }
    }
}
#[test]
fn dead_agents_leave_the_live_set_but_stay_as_panes() {
    let mut finished = agent("t1", "blocked");
    finished.state = "finished".into();
    let mut lost = agent("t2", "working");
    lost.pid = None;
    lost.reason = Some("holder_lost".into());
    let mut pending = agent("t3", "done");
    pending.pid = None;
    let view = view(vec![finished, lost, pending, agent("t4", "idle")], vec![]);
    let projected = project(&view, Some("t4"));
    let snapshot = &projected.snapshot;

    let agent_ids: Vec<&str> = snapshot.agents.iter().map(|a| a.pane_id.as_str()).collect();
    assert_eq!(
        agent_ids,
        ["t4"],
        "last reports of finished/lost/pending programs are not live"
    );
    // …but inventory is retained: all four still have panes.
    let pane_ids: Vec<&str> = snapshot.panes.iter().map(|p| p.pane_id.as_str()).collect();
    assert_eq!(pane_ids, ["t1", "t2", "t3", "t4"]);
}
#[test]
fn unknown_lifecycle_words_are_not_finished() {
    let mut odd = agent("t1", "working");
    odd.state = "suspended".into();
    let view = view(vec![odd], vec![]);
    let projected = project(&view, Some("t1"));
    // `suspended` is not `finished`: it is not an agent row, but the
    // terminal itself stays as a pane.
    assert!(projected.snapshot.agents.is_empty());
    assert_eq!(projected.snapshot.panes.len(), 1);
}
#[test]
fn subscription_loss_stales_everything_but_keeps_inventory() {
    let mut v = view(
        vec![agent("host-b/t1", "working")],
        vec![machine("host-b", "connected")],
    );
    v.link = Link::Offline {
        attempt: 3,
        error: "gone".into(),
    };
    v.inventory.set_offline(true);
    let projected = project(&v, Some("host-b/t1"));

    for e in &projected.endpoints {
        assert_ne!(
            e.status,
            ClientEndpointStatus::Online,
            "{:?} must be stale on stream loss",
            e.endpoint_id
        );
        assert!(
            e.snapshot.is_some(),
            "{:?} retains its frame while reconnecting",
            e.endpoint_id
        );
    }
    let host_b = endpoint(&projected, "host-b");
    assert!(
        host_b
            .snapshot
            .as_deref()
            .unwrap()
            .panes
            .iter()
            .any(|p| p.pane_id == "host-b/t1"),
        "cached remote terminal is retained, presented stale"
    );
}
#[test]
fn rpc_loss_stales_machine_state_but_keeps_terminals_live() {
    // The old `connected·stale` badge maps to Attention: the machine's
    // own state word is the stale part while terminal rows still
    // stream over the subscription.
    let mut v = view(
        vec![agent("host-b/t1", "working")],
        vec![machine("host-b", "connected")],
    );
    v.rpc = Link::Offline {
        attempt: 1,
        error: "rpc gone".into(),
    };
    let projected = project(&v, Some("t1"));

    let host_b = endpoint(&projected, "host-b");
    assert_eq!(host_b.status, ClientEndpointStatus::Attention);
    let agent_row = host_b
        .snapshot
        .as_deref()
        .unwrap()
        .agents
        .iter()
        .find(|a| a.pane_id == "host-b/t1")
        .expect("its terminals still stream over the subscription");
    assert_eq!(agent_row.agent_status, AgentStatus::Working);
    // …and the local lane is untouched.
    assert_eq!(
        endpoint(&projected, "local").status,
        ClientEndpointStatus::Online
    );
}
#[test]
fn offline_presents_reconnecting_and_retains_distinct_lifecycles() {
    // finished/lost/pending are distinct from live agents regardless of
    // the link: they never appear as agents, they never lose the pane.
    let mut finished = agent("t1", "done");
    finished.state = "finished".into();
    finished.exit_code = Some(0);
    let mut lost = agent("t2", "working");
    lost.pid = None;
    lost.reason = Some("holder_lost".into());
    let mut pending = agent("t3", "working");
    pending.pid = None;
    pending.reason = Some("spawn_pending".into());
    let mut v = view(
        vec![finished, lost, pending, agent("t4", "blocked")],
        vec![],
    );
    v.link = Link::Offline {
        attempt: 2,
        error: "socket closed".into(),
    };
    v.inventory.set_offline(true);
    let projected = project(&v, Some("t4"));

    let snapshot = &projected.snapshot;
    // The only live agent reports its last status — only the endpoint
    // dims, not the word itself.
    let t4 = snapshot
        .agents
        .iter()
        .find(|a| a.pane_id == "t4")
        .expect("blocked agent still reports");
    assert_eq!(t4.agent_status, AgentStatus::Blocked);
    assert_eq!(snapshot.agents.len(), 1);
    assert_eq!(snapshot.panes.len(), 4, "retained inventory while down");
    assert_eq!(
        endpoint(&projected, "local").status,
        ClientEndpointStatus::Reconnecting
    );
}
#[test]
fn an_unlisted_machine_prefix_gets_its_own_endpoint() {
    let view = view(vec![agent("host-z/t1", "idle")], vec![]);
    let projected = project(&view, Some("host-z/t1"));
    let host_z = endpoint(&projected, "host-z");
    // No `machine.list` row: the state word is unknown, so the endpoint
    // presents attention and its snapshot carries the stray terminal.
    assert_eq!(host_z.status, ClientEndpointStatus::Attention);
    assert!(
        host_z
            .snapshot
            .as_deref()
            .unwrap()
            .panes
            .iter()
            .any(|p| p.pane_id == "host-z/t1")
    );
}
#[test]
fn every_rendered_string_is_sanitized() {
    let mut t = agent("t1", "working");
    t.name = Some("bad\x1b[2Jname".into());
    t.title = "evil\x1b]8;;x\x07title".into();
    t.cwd = "/home/user/\x1bdir".into();
    t.labels
        .insert("herdr.space".into(), "sp\x1b[31mace".into());
    let mut dirty = agent("t2", "working");
    dirty.name = Some("tricky\x07".into());
    let mut m = machine("host-b", "unreachable");
    m.why = Some("why\x1b!".into());
    let mut remote = agent("host-b/t1", "working");
    remote.name = Some("rem\x07ote".into());
    let mut v = view(vec![t, dirty, remote], vec![m]);
    v.link = Link::Offline {
        attempt: 1,
        error: "oops\x1b[31mred".into(),
    };
    let projected = project(&v, Some("t1"));

    let check = |s: &str| assert!(!s.chars().any(|c| c.is_control()), "{s:?}");
    for e in &projected.endpoints {
        check(&e.label);
        let Some(snapshot) = e.snapshot.as_deref() else {
            continue;
        };
        for w in &snapshot.workspaces {
            check(&w.label);
            check(&w.new_workspace_cwd);
        }
        for tab in &snapshot.tabs {
            check(&tab.label);
        }
        for p in &snapshot.panes {
            p.label.as_deref().map(&check);
            p.cwd.as_deref().map(&check);
            p.foreground_cwd.as_deref().map(&check);
        }
        for a in &snapshot.agents {
            for f in [
                &a.name,
                &a.display_agent,
                &a.agent,
                &a.title,
                &a.terminal_title,
            ]
            .into_iter()
            .flatten()
            {
                check(f);
            }
        }
    }
}

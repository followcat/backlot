use super::*;
use crate::chrome::status::AgentStatus;
use crate::sidebar::fixture::{agent, view};
use hc_proto::terminal::{Status, Terminal as CoreTerminal};

fn record(state: &str, app: &str, title: &str) -> Status {
    Status {
        state: state.into(),
        app: app.into(),
        title: title.into(),
        ..Status::default()
    }
}

fn app_with(term: CoreTerminal) -> App {
    let (accepted, _rx) = mpsc::channel(1);
    let (rpc, sub) = crate::testcore::openers(accepted);
    let watcher = Watcher::with_openers(rpc, sub, hc_proto::watch::WatchParams::default());
    let open: OpenAttach = Arc::new(|_| unreachable!("no attach in this test"));
    let config = crate::config::Config::default();
    let shell = ClientShellConfig::from_config(&config, None, None, std::path::PathBuf::new());
    let mut app = App::new(
        watcher,
        open,
        config.live_keybinds(),
        shell,
        Arc::new(Probe::default()),
        Box::new(|_, _| {}),
    );
    app.view = view(vec![term], vec![]);
    app.refresh_projection();
    app
}

fn replace(app: &mut App, generation: u64, term: &CoreTerminal) {
    app.view.inventory.replace(generation, vec![term.clone()]);
    app.refresh_projection();
}

#[tokio::test]
async fn agent_border_uses_the_same_selected_record_as_the_snapshot() {
    let mut term = agent("t1", "working");
    term.status = Some(record("working", "root-app", "root-title"));
    term.records
        .insert("child".into(), record("error", "child-app", "child-title"));
    term.labels
        .insert("herdr.title".into(), "manual-title".into());
    let mut app = app_with(term.clone());
    let row = &app.projection.snapshot.agents[0];
    assert_eq!(row.agent_status, AgentStatus::Working);
    assert_eq!(row.agent.as_deref(), Some("root-app"));
    assert_eq!(row.title.as_deref(), Some("root-title"));
    assert_eq!(border_label(&app, "t1").as_deref(), Some("root-title"));

    term.status.as_mut().unwrap().title.clear();
    replace(&mut app, 2, &term);
    assert_eq!(border_label(&app, "t1").as_deref(), Some("manual-title"));

    term.state = "finished".into();
    term.pid = None;
    replace(&mut app, 3, &term);
    assert!(app.projection.snapshot.agents.is_empty());
    assert_eq!(border_label(&app, "t1").as_deref(), Some("child-title"));
}

#[tokio::test]
async fn unselected_completion_is_retained_and_acknowledged_for_the_agent() {
    let mut term = agent("t1", "working");
    term.status = Some(record("working", "root-app", "root-title"));
    term.records
        .insert("child".into(), record("idle", "child-app", "child-title"));
    term.activity = Some(
        serde_json::from_value(serde_json::json!({
            "core_id": "origin", "epoch": "boot", "scope": 1, "revision": 1,
            "records": {
                "": {"state_ordinal": 1, "completion_ordinal": null},
                "child": {"state_ordinal": 2, "completion_ordinal": null}
            }
        }))
        .unwrap(),
    );
    let mut app = app_with(term.clone());
    let child = term
        .activity
        .as_mut()
        .unwrap()
        .records
        .get_mut("child")
        .unwrap();
    child.state_ordinal = Some(11);
    child.completion_ordinal = Some(11);
    replace(&mut app, 2, &term);
    assert_eq!(
        app.projection.snapshot.agents[0].agent_status,
        AgentStatus::Working
    );
    assert!(
        !app.presentation
            .acknowledge(&mut app.projection, "t1", Some(false))
    );

    term.status.as_mut().unwrap().state = "idle".into();
    replace(&mut app, 3, &term);
    let row = &app.projection.snapshot.agents[0];
    assert_eq!(row.agent_status, AgentStatus::Done);
    assert_eq!(row.agent.as_deref(), Some("root-app"));
    assert_eq!(border_label(&app, "t1").as_deref(), Some("root-title"));
    assert!(
        app.presentation
            .acknowledge(&mut app.projection, "t1", Some(true))
    );
    assert_eq!(
        app.projection.snapshot.agents[0].agent_status,
        AgentStatus::Idle
    );

    term.status.as_mut().unwrap().state = "working".into();
    let child = term
        .activity
        .as_mut()
        .unwrap()
        .records
        .get_mut("child")
        .unwrap();
    child.state_ordinal = Some(12);
    child.completion_ordinal = Some(12);
    replace(&mut app, 4, &term);
    assert_eq!(
        app.projection.snapshot.agents[0].agent_status,
        AgentStatus::Working
    );
    assert!(
        app.presentation
            .acknowledge(&mut app.projection, "t1", Some(true))
    );
    term.status.as_mut().unwrap().state = "idle".into();
    replace(&mut app, 5, &term);
    assert_eq!(
        app.projection.snapshot.agents[0].agent_status,
        AgentStatus::Idle
    );
}

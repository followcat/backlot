use super::*;
use crate::chrome::status::AgentStatus;
use hc_proto::watch::ActivityReceipts;
use serde_json::{Value, json};

fn raw() -> Value {
    json!({
        "address":"t1","labels":{},"argv":["fixture"],"cwd":"/home/user/acme-shop",
        "state":"live","pid":100,"created_ms":1,"screen":"exact","cols":80,"rows":24,
        "reported":true,"status":{"state":"idle","app":"fixture"},
        "activity":{"core_id":"host-a","epoch":"boot","scope":1,"revision":1,
            "records":{"":{"state_ordinal":1,"completion_ordinal":null}}}
    })
}

fn update(
    p: &mut Presentation,
    r: &mut ActivityReceipts,
    values: Vec<Value>,
) -> (View, Projection) {
    let mut view = View {
        generation: 1,
        ..View::default()
    };
    view.inventory.replace(
        1,
        values
            .into_iter()
            .map(|v| serde_json::from_value(v).unwrap())
            .collect(),
    );
    r.ingest(&view.inventory);
    view.activity_receipts = r.clone();
    let mut projection = crate::project::project(&view, Some("t1"));
    p.project(&view, &mut projection);
    (view, projection)
}

fn state(projection: &Projection) -> AgentStatus {
    projection
        .snapshot
        .agents
        .iter()
        .find(|a| a.pane_id == "t1")
        .unwrap()
        .agent_status
}

#[test]
fn complete_withdrawal_removes_agent_rank_and_presentation_but_keeps_pane() {
    let (mut p, mut r) = (Presentation::default(), ActivityReceipts::default());
    let mut value = raw();
    update(&mut p, &mut r, vec![value.clone()]);
    value["activity"]["revision"] = json!(5);
    value["activity"]["records"][""] = json!({"state_ordinal":5,"completion_ordinal":5});
    assert_eq!(
        state(&update(&mut p, &mut r, vec![value.clone()]).1),
        AgentStatus::Done
    );
    value["status"] = Value::Null;
    value["activity"]["revision"] = json!(6);
    value["activity"]["records"][""] = json!({"state_ordinal":6,"completion_ordinal":null});
    let (view, mut projection) = update(&mut p, &mut r, vec![value.clone()]);
    assert!(projection.snapshot.agents.is_empty());
    assert!(projection.snapshot.agent_order.is_empty());
    assert!(
        projection
            .endpoints
            .iter()
            .all(|e| e.agent_recency.is_empty())
    );
    assert!(p.agents.is_empty());
    assert_eq!(r.rank("t1"), 0);
    assert_eq!(projection.snapshot.panes[0].pane_id, "t1");
    assert_eq!(projection.snapshot.tabs.len(), 1);
    assert!(!projection.layouts.is_empty());
    assert!(view.inventory.get("t1").unwrap().reported);
    assert_eq!(
        serde_json::to_value(view.inventory.get("t1").unwrap().activity.as_ref().unwrap()).unwrap(),
        value["activity"]
    );
    assert!(!p.acknowledge(&mut projection, "t1", Some(true)));
    value["status"] = json!({"state":"idle","app":"fixture-b"});
    value["activity"]["revision"] = json!(7);
    value["activity"]["records"][""] = json!({"state_ordinal":7,"completion_ordinal":null});
    let (_, ready) = update(&mut p, &mut r, vec![value.clone()]);
    assert_eq!(state(&ready), AgentStatus::Idle);
    assert_eq!(ready.snapshot.agents[0].agent.as_deref(), Some("fixture-b"));
    value["activity"]["records"][""] = json!({"state_ordinal":8,"completion_ordinal":8});
    value["activity"]["revision"] = json!(8);
    assert_eq!(
        state(&update(&mut p, &mut r, vec![value]).1),
        AgentStatus::Done
    );
}

#[test]
fn explicit_unknown_root_and_named_only_status_project_without_app_or_reported() {
    for named in [false, true] {
        let (mut p, mut r) = (Presentation::default(), ActivityReceipts::default());
        let mut value = raw();
        value["reported"] = json!(false);
        value["status"] = json!({"state":"unknown","app":""});
        if named {
            value["status"] = Value::Null;
            value["records"] = json!({"build/test":{"state":"unknown","app":""}});
        }
        let (_, projection) = update(&mut p, &mut r, vec![value]);
        assert_eq!(state(&projection), AgentStatus::Unknown);
        assert_eq!(r.rank("t1"), 1);
    }
}

#[test]
fn removing_named_completion_preserves_independent_root_identity() {
    let (mut p, mut r) = (Presentation::default(), ActivityReceipts::default());
    let mut value = raw();
    update(&mut p, &mut r, vec![value.clone()]);
    value["records"] = json!({"build/test":{"state":"idle","app":"fixture-b"}});
    value["activity"]["revision"] = json!(20);
    value["activity"]["records"]["build/test"] =
        json!({"state_ordinal":20,"completion_ordinal":20});
    assert_eq!(
        state(&update(&mut p, &mut r, vec![value.clone()]).1),
        AgentStatus::Done
    );
    value["records"] = json!({});
    value["activity"]["revision"] = json!(21);
    value["activity"]["records"]["build/test"] =
        json!({"state_ordinal":21,"completion_ordinal":null});
    let (_, projection) = update(&mut p, &mut r, vec![value]);
    assert_eq!(state(&projection), AgentStatus::Idle);
    assert_eq!(
        projection.snapshot.agents[0].agent.as_deref(),
        Some("fixture")
    );
    assert!(p.agents.contains_key("t1"));
}

#[test]
fn same_address_incarnation_baselines_old_ack_and_has_a_new_receipt() {
    let (mut p, mut r) = (Presentation::default(), ActivityReceipts::default());
    let mut a = raw();
    let mut b = raw();
    b["address"] = json!("t2");
    b["activity"]["core_id"] = json!("host-b");
    update(&mut p, &mut r, vec![a.clone(), b.clone()]);
    a["activity"]["revision"] = json!(5);
    a["activity"]["records"][""] = json!({"state_ordinal":5,"completion_ordinal":5});
    let (_, mut done) = update(&mut p, &mut r, vec![a.clone(), b.clone()]);
    assert_eq!(state(&done), AgentStatus::Done);
    assert!(p.acknowledge(&mut done, "t1", Some(true)));
    a["activity"]["revision"] = json!(6);
    a["activity"]["records"][""] = json!({"state_ordinal":6,"completion_ordinal":6});
    assert_eq!(
        state(&update(&mut p, &mut r, vec![a.clone(), b.clone()]).1),
        AgentStatus::Done
    );
    let old_rank = r.rank("t1");
    a["created_ms"] = json!(2);
    a["pid"] = json!(101);
    assert_eq!(
        state(&update(&mut p, &mut r, vec![a.clone(), b.clone()]).1),
        AgentStatus::Idle
    );
    assert!(r.rank("t1") > old_rank && r.rank("t1") > r.rank("t2"));
    a["activity"]["revision"] = json!(7);
    a["activity"]["records"][""] = json!({"state_ordinal":7,"completion_ordinal":7});
    assert_eq!(
        state(&update(&mut p, &mut r, vec![a.clone(), b.clone()]).1),
        AgentStatus::Done
    );
    a["created_ms"] = json!(3);
    a["pid"] = json!(102);
    a["status"] = Value::Null;
    a["records"] = json!({});
    a["activity"]["revision"] = json!(8);
    a["activity"]["records"][""] = json!({"state_ordinal":8,"completion_ordinal":null});
    let (view, projection) = update(&mut p, &mut r, vec![a.clone(), b]);
    assert!(
        projection
            .snapshot
            .agents
            .iter()
            .all(|agent| agent.pane_id != "t1")
    );
    assert!(!projection.snapshot.agent_order.iter().any(|id| id == "t1"));
    assert!(
        projection
            .endpoints
            .iter()
            .all(|endpoint| !endpoint.agent_recency.contains_key("t1"))
    );
    assert!(!p.agents.contains_key("t1"));
    assert_eq!(r.rank("t1"), 0);
    assert_eq!(r.rank("t2"), 2);
    let term = view.inventory.get("t1").unwrap();
    assert_eq!((term.created_ms, term.pid), (3, Some(102)));
    assert!(term.reported && term.status.is_none() && term.records.is_empty());
    assert_eq!(
        serde_json::to_value(term.activity.as_ref().unwrap()).unwrap(),
        a["activity"]
    );
}

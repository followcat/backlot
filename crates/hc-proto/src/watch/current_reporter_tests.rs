use super::*;
use serde_json::{Value, json};

fn raw() -> Value {
    // Core keeps reported/activity after full withdrawal; empty records may be omitted.
    json!({
        "address":"t1","labels":{"herdr.space":"work"},"argv":["fixture"],
        "cwd":"/home/user/acme-shop","state":"live","pid":100,"created_ms":1,
        "screen":"exact","cols":80,"rows":24,"reported":true,"status":null,
        "name":"fixture","title":"fixture",
        "activity":{"core_id":"host-a","epoch":"boot","scope":1,"revision":6,
            "records":{"":{"state_ordinal":6,"completion_ordinal":null}}}
    })
}

fn ingest(receipts: &mut ActivityReceipts, values: Vec<Value>) -> Inventory {
    let mut inventory = Inventory::default();
    inventory.replace(
        1,
        values
            .into_iter()
            .map(|v| serde_json::from_value(v).unwrap())
            .collect(),
    );
    receipts.ingest(&inventory);
    inventory
}

#[test]
fn historical_reported_and_tombstones_do_not_create_membership() {
    for explicit_empty in [false, true] {
        let mut value = raw();
        if explicit_empty {
            value["records"] = json!({});
        } else {
            value.as_object_mut().unwrap().remove("status");
        }
        let mut receipts = ActivityReceipts::default();
        let inventory = ingest(&mut receipts, vec![value.clone()]);
        let term = inventory.get("t1").unwrap();
        assert!(!term.is_live_reporter());
        assert_eq!(receipts.rank("t1"), 0);
        assert!(receipts.observed.is_empty());
        assert!(term.reported && term.records.is_empty());
        assert_eq!(
            serde_json::to_value(term.activity.as_ref().unwrap()).unwrap(),
            value["activity"]
        );
    }
}

#[test]
fn explicit_unknown_root_or_named_status_needs_no_app_or_reported_flag() {
    for named in [false, true] {
        let mut value = raw();
        value["reported"] = json!(false);
        if named {
            value["records"] = json!({"build/test":{"state":"unknown","app":""}});
        } else {
            value["status"] = json!({"state":"unknown","app":""});
        }
        let mut receipts = ActivityReceipts::default();
        let inventory = ingest(&mut receipts, vec![value]);
        assert!(inventory.get("t1").unwrap().is_live_reporter());
        assert_eq!(receipts.rank("t1"), 1);
    }
}

#[test]
fn full_withdrawal_prunes_receipts_and_reacquisition_allocates_after_peer() {
    let mut a = raw();
    a["status"] = json!({"state":"idle","app":"fixture"});
    a["activity"]["records"][""] = json!({"state_ordinal":5,"completion_ordinal":5});
    a["activity"]["revision"] = json!(5);
    let mut b = a.clone();
    b["address"] = json!("t2");
    b["activity"]["core_id"] = json!("host-b");
    let mut receipts = ActivityReceipts::default();
    ingest(&mut receipts, vec![a.clone(), b.clone()]);
    assert_eq!((receipts.rank("t1"), receipts.rank("t2")), (1, 2));
    let inventory = ingest(&mut receipts, vec![raw(), b.clone()]);
    assert!(inventory.get("t1").unwrap().reported);
    assert_eq!(receipts.rank("t1"), 0);
    assert_eq!(receipts.observed.len(), 1);
    a["activity"]["revision"] = json!(7);
    a["activity"]["records"][""] = json!({"state_ordinal":7,"completion_ordinal":null});
    ingest(&mut receipts, vec![a, b]);
    assert_eq!((receipts.rank("t1"), receipts.rank("t2")), (3, 2));
}

#[test]
fn present_status_does_not_bypass_live_pid_bounds() {
    for reason in ["finished", "holder_lost", "spawn_pending", ""] {
        let mut value = raw();
        value["status"] = json!({"state":"working"});
        if reason == "finished" {
            value["state"] = json!("finished");
        } else {
            value["pid"] = Value::Null;
            value["reason"] = if reason.is_empty() {
                Value::Null
            } else {
                json!(reason)
            };
        }
        let mut receipts = ActivityReceipts::default();
        let inventory = ingest(&mut receipts, vec![value]);
        assert!(!inventory.get("t1").unwrap().is_live_reporter());
        assert_eq!(receipts.rank("t1"), 0);
    }
}

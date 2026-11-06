use super::*;
use crate::events::{EventParams, ServerNotification};
use crate::terminal::Terminal;
use serde_json::{Value, json};

fn actor() -> Actor {
    let (version, _) = watch::channel(0);
    let (_, commands) = mpsc::channel(1);
    let (_, stop) = watch::channel(false);
    Actor::new(
        WatchParams::default(),
        Arc::new(Mutex::new(View::default())),
        version,
        commands,
        stop,
        Arc::new(|| Box::pin(async { panic!("no RPC transport is opened") })),
        Arc::new(|_| Box::pin(async { panic!("no subscription transport is opened") })),
    )
}

fn term(id: &str, epoch: &str, n: u64) -> Terminal {
    serde_json::from_value(json!({
        "address":id,"labels":{},"argv":["fixture"],"cwd":"/home/user/acme-shop",
        "state":"live","pid":100,"created_ms":1,"screen":"exact","cols":80,"rows":24,
        "reported":true,"status":{"state":"idle","app":"fixture"},
        "activity":{"core_id":"host-a","epoch":epoch,"scope":1,"revision":n,
            "records":{"":{"state_ordinal":n,"completion_ordinal":null}}}
    }))
    .unwrap()
}

fn activity_event(seq: u64, id: &str, activity: Value) -> ServerNotification {
    ServerNotification::Event(EventParams {
        seq,
        terminal: Some(id.into()),
        kind: "activity".into(),
        machine: None,
        data: json!({"activity": activity}),
    })
}

#[test]
fn a_coalesced_epoch_boundary_and_later_work_each_reallocate() {
    let mut actor = actor();
    actor
        .inventory
        .replace(1, vec![term("a", "boot", 1), term("b", "peer", 5)]);
    actor.publish();
    // A genuine epoch boundary lands, then real post-boundary work — both
    // ingested before any consumer reads the published view.
    let boundary = serde_json::to_value(term("a", "boot-2", 4).activity.unwrap()).unwrap();
    actor.notification(Ok(activity_event(2, "a", boundary)));
    let work = serde_json::to_value(term("a", "boot-2", 9).activity.unwrap()).unwrap();
    actor.notification(Ok(activity_event(3, "a", work)));
    let view = actor.view.lock().unwrap().clone();
    // The boundary itself reallocates like a changed fact; the later
    // same-epoch work reallocates once more. The raw ordinals are untouched.
    assert_eq!(
        (
            view.activity_receipts.rank("a"),
            view.activity_receipts.rank("b")
        ),
        (4, 2)
    );
    let raw = view.inventory.get("a").unwrap();
    assert_eq!(raw.activity.as_ref().unwrap().epoch, "boot-2");
    assert_eq!(
        raw.activity.as_ref().unwrap().records[""].state_ordinal,
        Some(9)
    );
}

#[test]
fn publish_orders_boundary_reallocation_by_the_resolved_traversal() {
    let mut actor = actor();
    // The pre-publication seam: resolve the traversal on the candidate
    // view — here, reversed incoming order — before receipts ingest.
    actor.params.traversal = Some(|view| {
        let mut order: Vec<String> = view
            .inventory
            .ordered_terminals()
            .map(|term| term.address.clone())
            .collect();
        order.reverse();
        order
    });
    actor
        .inventory
        .replace(1, vec![term("a", "boot", 1), term("b", "peer", 5)]);
    actor.publish();
    // Both boundaries land in ONE replacement snapshot: traversal [b, a]
    // allocates b before a, opposite of incoming order.
    actor
        .inventory
        .replace(2, vec![term("a", "boot-2", 4), term("b", "peer-2", 6)]);
    actor.publish();
    let view = actor.view.lock().unwrap().clone();
    assert!(
        view.activity_receipts.rank("b") < view.activity_receipts.rank("a"),
        "resolved traversal must order the boundary reallocation"
    );
}

#[test]
fn an_unchanged_epoch_in_a_replacement_snapshot_keeps_every_receipt() {
    let mut actor = actor();
    actor
        .inventory
        .replace(1, vec![term("a", "boot", 1), term("b", "peer", 5)]);
    actor.publish();
    actor
        .inventory
        .replace(2, vec![term("a", "boot", 1), term("b", "peer", 5)]);
    actor.publish();
    let view = actor.view.lock().unwrap().clone();
    assert_eq!(
        (
            view.activity_receipts.rank("a"),
            view.activity_receipts.rank("b")
        ),
        (1, 2)
    );
}

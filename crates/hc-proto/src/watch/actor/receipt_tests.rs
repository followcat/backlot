use super::*;
use crate::events::{EventParams, MissedParams, ServerNotification};
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

fn term(id: &str, origin: &str, n: u64) -> Terminal {
    serde_json::from_value(json!({
        "address":id,"labels":{},"argv":["fixture"],"cwd":"/home/user/acme-shop",
        "state":"live","pid":100,"created_ms":1,"screen":"exact","cols":80,"rows":24,
        "reported":true,"status":{"state":"idle","app":"fixture"},
        "activity":{"core_id":origin,"epoch":"boot","scope":1,"revision":n,
            "records":{"":{"state_ordinal":n,"completion_ordinal":null}}}
    }))
    .unwrap()
}

fn event(seq: u64, id: &str, kind: &str, data: Value) -> ServerNotification {
    ServerNotification::Event(EventParams {
        seq,
        terminal: Some(id.into()),
        kind: kind.into(),
        machine: None,
        data,
    })
}

#[test]
fn coalesced_publication_keeps_a20_b_a10_without_an_intermediate_view_read() {
    let mut actor = actor();
    actor.inventory.replace(1, vec![term("a20", "a", 20)]);
    actor.publish();
    actor.notification(Ok(event(2, "b", "created", json!(term("b", "b", 1)))));
    actor.notification(Ok(event(3, "a10", "created", json!(term("a10", "a", 10)))));
    // The consumer reads only the final publish, after all three ingestions.
    let view = actor.view.lock().unwrap().clone();
    assert_eq!(
        (
            view.activity_receipts.rank("a20"),
            view.activity_receipts.rank("b"),
            view.activity_receipts.rank("a10")
        ),
        (1, 2, 3)
    );
    assert_eq!(
        view.inventory
            .get("a20")
            .unwrap()
            .activity
            .as_ref()
            .unwrap()
            .records[""]
            .state_ordinal,
        Some(20)
    );
    assert_eq!(
        view.inventory
            .get("a10")
            .unwrap()
            .activity
            .as_ref()
            .unwrap()
            .records[""]
            .state_ordinal,
        Some(10)
    );
    actor.notification(Ok(ServerNotification::Missed(MissedParams {
        seq: 3,
        machine: None,
        terminals: vec![
            term("a10", "a", 10),
            term("b", "b", 1),
            term("a20", "a", 20),
        ],
    })));
    let view = actor.view.lock().unwrap().clone();
    assert_eq!(
        (
            view.activity_receipts.rank("a20"),
            view.activity_receipts.rank("b"),
            view.activity_receipts.rank("a10")
        ),
        (1, 2, 3)
    );
}

#[test]
fn snapshot_input_order_survives_the_address_map_and_unchanged_reconnect() {
    let mut actor = actor();
    let terms = vec![term("z", "z", u64::MAX), term("a", "a", 1)];
    actor.inventory.replace(5, terms.clone());
    actor.publish();
    actor.generation += 1;
    actor
        .inventory
        .replace(0, terms.into_iter().rev().collect());
    actor.publish();
    let view = actor.view.lock().unwrap().clone();
    assert_eq!(
        (
            view.activity_receipts.rank("z"),
            view.activity_receipts.rank("a")
        ),
        (1, 2)
    );
}

#[test]
fn duplicate_malformed_and_stale_events_cannot_allocate_a_receipt() {
    let mut actor = actor();
    actor.inventory.replace(5, vec![term("a", "a", 1)]);
    actor.publish();
    actor.notification(Ok(event(4, "z", "created", json!(term("z", "z", 1)))));
    actor.notification(Ok(event(6, "a", "status", json!({"status":null}))));
    assert!(actor.inventory.is_stale());
    actor.notification(Ok(event(7, "z", "created", json!(term("z", "z", 1)))));
    let view = actor.view.lock().unwrap().clone();
    assert_eq!(
        (
            view.activity_receipts.rank("a"),
            view.activity_receipts.rank("z")
        ),
        (1, 0)
    );
    // A malformed stream is closed. Recovery installs a fresh subscription's
    // inventory, rather than accepting a notification from the dead stream.
    actor.inventory = Inventory::default();
    actor
        .inventory
        .replace(7, vec![term("a", "a", 1), term("z", "z", 1)]);
    actor.generation += 1;
    actor.publish();
    let view = actor.view.lock().unwrap().clone();
    assert_eq!(
        (
            view.activity_receipts.rank("a"),
            view.activity_receipts.rank("z")
        ),
        (1, 2)
    );
}

#[test]
fn unseen_restarted_reporter_and_its_later_activity_receive_receipts() {
    let mut actor = actor();
    actor.inventory.replace(1, vec![]);
    actor.publish();
    actor.notification(Ok(event(2, "a", "restarted", json!(term("a", "a", 1)))));
    assert_eq!(actor.view.lock().unwrap().activity_receipts.rank("a"), 1);
    let changed = term("a", "a", 2);
    actor.notification(Ok(event(
        3,
        "a",
        "activity",
        json!({"activity": changed.activity.as_ref().unwrap()}),
    )));
    assert_eq!(actor.view.lock().unwrap().activity_receipts.rank("a"), 2);
}

#[test]
fn fresh_static_fallback_after_subscription_loss_updates_and_prunes_receipts() {
    let mut actor = actor();
    actor
        .inventory
        .replace(1, vec![term("a", "a", 1), term("b", "b", 1)]);
    actor.link = Link::Online;
    actor.publish();
    actor.notification(Ok(ServerNotification::Heartbeat(
        serde_json::from_value(json!({"seq":9})).unwrap(),
    )));
    assert!(actor.inventory.is_offline());
    actor.install_sub(Ok(SubAttempt::Unsupported("events unavailable".into())));
    actor.static_done(Ok((
        actor.rpc_epoch,
        Ok(ListResult {
            terminals: vec![term("a", "a", 2), term("c", "c", 1)],
            hidden_finished: 0,
            machines: vec![],
        }),
    )));
    let view = actor.view.lock().unwrap().clone();
    assert!(!view.inventory.is_offline());
    assert_eq!(
        (
            view.activity_receipts.rank("a"),
            view.activity_receipts.rank("c")
        ),
        (3, 4)
    );
    assert_eq!(view.activity_receipts.rank("b"), 0);
}

#[test]
fn current_reporter_withdrawal_is_ingested_before_a_coalesced_reacquisition() {
    let mut actor = actor();
    actor
        .inventory
        .replace(1, vec![term("a", "host-a", 1), term("b", "host-b", 1)]);
    actor.publish();
    let mut active = serde_json::to_value(term("a", "host-a", 2)).unwrap();
    active["status"] = Value::Null;
    actor.notification(Ok(event(
        2,
        "a",
        "status",
        json!({
            "status":null,"records":{},"reported":true,"activity":active["activity"]
        }),
    )));
    assert!(!actor.inventory.is_stale());
    assert_eq!(actor.activity_receipts.rank("a"), 0);
    let active = serde_json::to_value(term("a", "host-a", 3)).unwrap();
    actor.notification(Ok(event(
        3,
        "a",
        "status",
        json!({
            "status":active["status"],"records":{},"reported":true,"activity":active["activity"]
        }),
    )));
    // No consumer read occurs between withdrawal and actual status reacquisition.
    let view = actor.view.lock().unwrap().clone();
    assert_eq!(
        (
            view.activity_receipts.rank("a"),
            view.activity_receipts.rank("b")
        ),
        (3, 2)
    );
    let raw = view.inventory.get("a").unwrap();
    assert!(raw.reported);
    assert_eq!(
        serde_json::to_value(raw.activity.as_ref().unwrap()).unwrap(),
        active["activity"]
    );
}
